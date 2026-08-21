#![no_std]
// `initialize` takes 8 arguments, one over clippy's default threshold. The argument
// list of a `pub fn` in a #[contractimpl] block is the contract's on-chain ABI:
// changing it would change the generated bindings and force a redeploy to a new
// address. The lint has to be silenced at crate level rather than on the impl
// block, because #[contractimpl] re-emits the signature as sibling items that an
// item-level allow does not cover.
#![allow(clippy::too_many_arguments)]
// USDC amounts are written as <dollars>_<7 decimals>, e.g. 10_0000000 is $10.
// Clippy reads that as inconsistent grouping; the grouping is deliberate so the
// dollar figure stays legible.
#![allow(clippy::inconsistent_digit_grouping)]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum ProofType {
    // Kategori A — kontrat kendisi doğrular (trustless).
    //
    // `InsufficientReserve` settles through the full waterfall: its harm is the
    // shortfall, an amount the operator actually failed to commit.
    //
    // `BoundExceeded` and `ExpiredCertificate` are also trustless — the router
    // is the source of truth for both — but they settle in HYGIENE MODE only.
    // See the comment above `resolve` for why that is not a shortcut.
    InsufficientReserve,
    BoundExceeded,
    ExpiredCertificate,
    // Kategori B — arbiter verdict verir (on-chain kanıtlanamaz).
    FakeSignature,
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum Verdict {
    Pending,
    /// Proven at filing, still true at window close: admitted to the pro-rata
    /// settlement.
    ChallengeWins,
    /// Wrong at filing. The bond is forfeited — this is the only outcome that
    /// forfeits one.
    ChallengeFails,
    /// DESIGN-V2 §2. Right at filing, remedied by the operator before the
    /// window closed. The bond is returned **in full**, the certificate
    /// survives and nobody is slashed.
    Cured,
    /// An arbiter-gated claim (`FakeSignature`) the arbiter never ruled on
    /// before the window closed. The bond is returned in full: a claim nobody
    /// judged is not a claim the challenger got wrong.
    Unadjudicated,
}

#[contracttype]
#[derive(Clone)]
pub struct Challenge {
    pub challenger: Address,
    pub cert_id: u64,
    pub proof_type: ProofType,
    pub victim: Address,
    pub stake: i128,
    pub verdict: Verdict,
    /// Ledger time the claim was filed. DESIGN-V2 §2: the predicate is
    /// evaluated here, and this is the instant the recorded state belongs to.
    pub filed_at: u64,
    /// DESIGN-V2 §2. The predicate's value **at filing**, recorded once and
    /// never recomputed. What was true when the challenge was filed stays true,
    /// so an operator cannot flip it to false and pocket the bond.
    pub proven: bool,
    /// The harm the predicate quantified **at filing**, in stroops. This is the
    /// number the pro-rata settlement divides by; live state never resizes it.
    pub harm: i128,
    /// False only for a `FakeSignature` claim the arbiter has not ruled on yet.
    /// Every on-chain predicate adjudicates itself at filing.
    pub adjudicated: bool,
    /// True when `harm` was **stated by the arbiter** rather than computed by a
    /// predicate. The distinction decides how the number aggregates across a
    /// claim window — see `close_window`.
    pub arbitrated: bool,
}

/// DESIGN-V2 §1. The claim window opened by the first valid challenge against a
/// certificate.
///
/// A window is a *certificate-level* object, not a challenge-level one. That is
/// the whole point: settlement runs once, over every claim the window admitted,
/// so being first is worth nothing.
#[contracttype]
#[derive(Clone)]
pub struct ClaimWindow {
    pub cert_id: u64,
    pub opened_at: u64,
    pub closes_at: u64,
    /// Every claim filed against this certificate while the window was open,
    /// in filing order. The order is recorded for auditability only — no payout
    /// anywhere below reads it.
    pub claims: Vec<u64>,
}

// Storage lifetime — defect L2. Soroban archives instance and persistent
// entries whose TTL lapses, and reaching an archived entry aborts the
// transaction rather than returning a default. Nothing here extended any TTL.
// 17,280 ledgers is one day at the ~5s close; the full reasoning for 120 days
// of runway, a threshold at half of it, and who pays the rent lives on the same
// two constants in `contracts/registry/src/lib.rs`.
const LEDGERS_PER_DAY: u32 = 17_280;
pub const TTL_THRESHOLD: u32 = 60 * LEDGERS_PER_DAY;
pub const TTL_EXTEND_TO: u32 = 120 * LEDGERS_PER_DAY;

#[contracttype]
pub enum DataKey {
    Registry,
    AuditorStaking,
    ReserveVault,
    FeeEscrow,
    Token,
    Arbiter,
    /// The PaymentRouter, source of truth for `spent` and `post_expiry_spent`.
    /// Set once, after initialize — see `set_router`.
    Router,
    /// The PremiumVault, source of the step-4 premium forfeiture. Set once,
    /// after initialize — see `set_premium_vault`.
    PremiumVault,
    /// Where slashed stake goes, and the only place it may go.
    Treasury,
    MinStake,
    Challenge(u64),
    ChallengeCount,
    /// Sum of challenger bonds currently held and still owed back. Anything the
    /// contract holds above this is forfeited-bond surplus, which is what funds
    /// the hygiene bounty.
    BondsHeld,
    /// DESIGN-V2 §1. The open claim window for a certificate, if any. Keyed by
    /// `cert_id`, not by challenge id.
    Window(u64),
    /// Set once a certificate's window has closed with at least one admitted
    /// claim. The certificate is dead, its reserve has been spent and its
    /// allocation retired, so no second window may ever open on it.
    Settled(u64),
}

/// The challenger's fee, in basis points of **proven harm**.
///
/// Deliberately a share of harm and not of the auditor's stake. v1 paid 20% of
/// the live stake, which made hunting *auditors* profitable rather than hunting
/// *fraud*: the payout scaled with how much collateral the auditor happened to
/// have, not with what anyone lost. Anchoring it to harm means a challenge is
/// worth filing exactly in proportion to the damage it surfaces.
const CHALLENGER_FEE_BPS: i128 = 1_000; // 10%

/// Flat bounty for a proof that is true but that nobody can evidence harm for
/// (see `settle_hygiene`). Fixed, small, and never scaled by the auditor's
/// stake — its job is to pay for the gas of killing a dead certificate, not to
/// fund a hunting expedition.
const HYGIENE_BOUNTY: i128 = 10_0000000; // $10

/// DESIGN-V2 §1. How long a claim window stays open, in ledger seconds.
///
/// The first *valid* challenge against a certificate opens a window instead of
/// settling. Any party may file an additional claim against the same
/// certificate until it closes, and settlement then runs **once**, over all
/// admitted claims, paying pro rata when proven harm exceeds the collateral.
///
/// WHY THIS EXISTS. Settlement invalidates the certificate, drains the reserve
/// and retires the allocation, so under the old lifecycle the *first* settled
/// challenge foreclosed every honest claim behind it. A self-challenge for the
/// minimum bond, filed and resolved immediately, permanently destroyed the
/// coverage other victims were relying on. The waterfall already ensured the
/// attacker gained nothing by it — and the honest victims still lost
/// everything, which is just as fatal.
///
/// THE COST, STATED HONESTLY. A genuine victim now waits out the whole window
/// before being paid a stroop, even when theirs is the only claim ever filed.
/// That latency is real and it is the price of not letting a self-dealer
/// foreclose them. It belongs in the trust model as a known latency, not
/// hidden. 72 hours is DESIGN-V2 §1's proposal, not a researched number.
const CLAIM_WINDOW_SECONDS: u64 = 72 * 60 * 60;

/// DESIGN-V2 §7. How long after `expires_at` a payment is still treated as
/// ordinary business rather than evidence of an expired covenant.
///
/// Without it, one $1 payment a second after expiry — which a hostile
/// counterparty can induce by simply invoicing the agent — permanently kills an
/// honest certificate. The window is free post-expiry coverage a hostile
/// operator can plan around, which is a real cost; it is accepted because the
/// alternative is a cliff anybody can push an honest operator off for a dollar.
/// 24 hours is §7's proposal, not a researched number.
const GRACE_WINDOW_SECONDS: u64 = 24 * 60 * 60;

/// DESIGN-V2 §7. The de-minimis floor, in basis points **of the certificate's
/// own bound**: 0.1%.
///
/// A percentage rather than a flat amount, because a flat floor is irrelevant at
/// a $1M bound and fatal at a $1k one. Anchoring it to the bound means the band
/// of unprovable late payments it creates is bounded by the same number the
/// certificate already advertises to its counterparties.
const DE_MINIMIS_FLOOR_BPS: i128 = 10;

/// A structural mirror of `payment_router::PostExpiry`.
///
/// The ChallengeManager reads the router over `invoke_contract`, so it needs the
/// return type locally. A `#[contracttype]` struct is encoded as a map keyed by
/// field **name**, so this decodes the router's value as long as the names and
/// types match. It is duplicated rather than imported to keep the router out of
/// this crate's dependency graph — the contracts are deployed and upgraded
/// independently, and a compile-time dependency would not make the on-chain ABI
/// any more coupled than it already is. If you change `PostExpiry` in the
/// router, change it here; `expired_certificate_upholds_when_all_three_conditions_hold`
/// in the integration harness fails loudly if the two drift.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PostExpiry {
    pub total: i128,
    pub count: u32,
    pub max_payment: i128,
    pub max_payment_at: u64,
    pub first_at: u64,
}

#[contract]
pub struct ChallengeManager;

#[contractimpl]
impl ChallengeManager {
    pub fn initialize(
        env: Env,
        registry: Address,
        auditor_staking: Address,
        reserve_vault: Address,
        fee_escrow: Address,
        token: Address,
        arbiter: Address,
        treasury: Address,
        min_stake: i128,
    ) {
        if env.storage().instance().has(&DataKey::Registry) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage()
            .instance()
            .set(&DataKey::AuditorStaking, &auditor_staking);
        env.storage()
            .instance()
            .set(&DataKey::ReserveVault, &reserve_vault);
        env.storage()
            .instance()
            .set(&DataKey::FeeEscrow, &fee_escrow);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        // Named once, at initialize. No admin and no upgrade path: making the
        // treasury mutable would reopen the prize this whole waterfall exists to
        // remove.
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::MinStake, &min_stake);
        env.storage().instance().set(&DataKey::BondsHeld, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::ChallengeCount, &0u64);
        Self::bump_instance(&env);
    }

    /// Point this contract at the PaymentRouter.
    ///
    /// Separate from `initialize` for one reason: the argument list of a
    /// `pub fn` in a `#[contractimpl]` block is the contract's ABI, and
    /// `initialize` already has eight parameters that the deploy script and the
    /// committed bindings pass positionally. A ninth would break both at
    /// runtime for no gain, so the wiring is a second one-shot call instead.
    ///
    /// **Arbiter-authorized, and settable exactly once.** `initialize` is
    /// unauthenticated (a known defect, tested as such), and copying that here
    /// would be materially worse: whoever wins the race to name the router names
    /// the contract that reports `spent`, and a lying router can invalidate any
    /// certificate it likes. The arbiter is already a named trusted party at
    /// `initialize`, so requiring their signature grants no new trust and closes
    /// the race. There is no re-pointing, for the same reason the treasury
    /// cannot be re-pointed.
    pub fn set_router(env: Env, router: Address) {
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();
        if env.storage().instance().has(&DataKey::Router) {
            panic!("router_already_set");
        }
        env.storage().instance().set(&DataKey::Router, &router);
        Self::bump_instance(&env);
    }

    pub fn get_router(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Router)
            .expect("router_not_set")
    }

    /// Point this contract at the PremiumVault, which owns step 4 of the
    /// settlement waterfall.
    ///
    /// A second one-shot call rather than a tenth `initialize` argument, for
    /// exactly the reasons spelled out on `set_router`: `initialize`'s argument
    /// list is the on-chain ABI, the deploy script and the committed bindings
    /// pass it positionally, and widening it would break both at runtime.
    ///
    /// **Arbiter-authorized, and settable exactly once.** The vault named here
    /// is handed the certificate's forfeited premium and told where to send it,
    /// so whoever names it can name a contract that keeps the money. The arbiter
    /// is already a trusted party at `initialize`, so requiring their signature
    /// grants no new trust and closes the race. There is no re-pointing, for the
    /// same reason the treasury cannot be re-pointed.
    pub fn set_premium_vault(env: Env, premium_vault: Address) {
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();
        if env.storage().instance().has(&DataKey::PremiumVault) {
            panic!("premium_vault_already_set");
        }
        env.storage()
            .instance()
            .set(&DataKey::PremiumVault, &premium_vault);
        Self::bump_instance(&env);
    }

    pub fn get_premium_vault(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::PremiumVault)
            .expect("premium_vault_not_set")
    }

    /// The vault every reserve proof and every settlement payment reads.
    ///
    /// Exposed for the Registry, which needs it at `attest` (DESIGN-V2 §4) to
    /// check a certificate's reserve is funded *against the same contract this
    /// one will later measure the shortfall on*. Publishing the address rather
    /// than letting the Registry hold its own copy is what makes the two
    /// checks agree by construction instead of by convention: there is exactly
    /// one `DataKey::ReserveVault` in the deployment, and it is this one.
    pub fn get_reserve_vault(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::ReserveVault)
            .expect("reserve_vault_not_set")
    }

    /// Whether step 4 is live. False means a deployment that never called
    /// `set_premium_vault`, in which case settlement silently skips the premium
    /// step — see the comment on step 4 in `settle_fraud`.
    pub fn has_premium_vault(env: Env) -> bool {
        env.storage().instance().has(&DataKey::PremiumVault)
    }

    /// File a claim against a certificate. Anyone may; a bond keeps it honest.
    ///
    /// DESIGN-V2 §1 + §2. This call does three things that the pre-window
    /// lifecycle did not:
    ///
    /// 1. **It evaluates the predicate now, and records the answer.** Nothing
    ///    downstream ever recomputes whether the challenger was right — only
    ///    whether the operator has since fixed it. An operator who tops the
    ///    reserve back up during the window can no longer flip the predicate to
    ///    false and pocket the bond.
    /// 2. **A claim that is true at filing opens or joins a claim window**
    ///    rather than settling. The first one freezes the certificate; the rest
    ///    queue up behind it and are all paid together, pro rata, at close.
    /// 3. **A claim that is false at filing, with no window open, is rejected
    ///    on the spot** and its bond forfeited. It deliberately does not open a
    ///    window: if a wrong claim could freeze a certificate for 72 hours,
    ///    anybody could freeze any certificate for the price of the minimum
    ///    bond. Once a window *is* open a wrong claim is allowed to join it —
    ///    it changes nothing, it is rejected at close, and that is exactly what
    ///    §2 asks for when a claim is filed after a cure.
    ///
    /// `victim` is the counterparty to be compensated if the claim is upheld.
    pub fn challenge(
        env: Env,
        challenger: Address,
        cert_id: u64,
        proof_type: ProofType,
        victim: Address,
        stake: i128,
    ) -> u64 {
        challenger.require_auth();

        let min_stake: i128 = env.storage().instance().get(&DataKey::MinStake).unwrap();
        if stake < min_stake {
            panic!("stake_below_minimum");
        }
        // A settled certificate is dead: reserve spent, allocation retired,
        // status Invalid. A second window on it could only manufacture a payout
        // out of collateral that is no longer there.
        if env.storage().persistent().has(&DataKey::Settled(cert_id)) {
            panic!("certificate_already_settled");
        }

        let now = env.ledger().timestamp();
        let open_window: Option<ClaimWindow> =
            env.storage().persistent().get(&DataKey::Window(cert_id));
        // A window past its close is not a window: it has to be closed and
        // settled before anything else happens to this certificate. Refusing
        // here rather than silently opening a second window is what keeps
        // "settlement runs once, over all admitted claims" true.
        if let Some(w) = &open_window {
            if now >= w.closes_at {
                panic!("claim_window_closed");
            }
        }

        // ---- R1. Nothing may be proven against a certificate whose collateral
        // the protocol has already released ---------------------------------
        //
        // The Registry's `get_cert_settlement_deadline` is documented as "the
        // instant after which nothing can still be proven against this
        // certificate, and therefore the instant its collateral may unwind".
        // Both money contracts honour the second half of that sentence and
        // release at exactly this instant. Nothing honoured the first half, and
        // the gap was a High finding:
        //
        //   `reserve_shortfall` compares the certificate's IMMUTABLE claimed
        //   reserve against the vault's LIVE balance, and `release_to_operator`
        //   zeroes that balance at the deadline. So the moment the protocol
        //   itself invites the operator to reclaim their money, every
        //   honestly-completed certificate acquires a permanently true
        //   `InsufficientReserve` proof. Filing it costs gas, re-freezes the
        //   certificate — which traps the auditor's allocation, because
        //   `release_allocation` is a separate call the auditor has to make
        //   themselves — and 72 hours later sends the entire allocation to the
        //   treasury. Nothing went wrong on that certificate and nobody is
        //   compensated: it is free, total destruction of auditor capital.
        //
        // The rail is the deadline itself. A claim filed after it is settling
        // against a pot the protocol already promised away, so it is refused.
        //
        // WHY ALL PROOF TYPES AND NOT JUST `InsufficientReserve`. The deadline's
        // stated meaning is not proof-type-specific, and neither is the harm: a
        // `FakeSignature` or hygiene claim filed after it freezes and slashes
        // exactly the same already-released collateral. Special-casing one
        // predicate would leave the same free freeze available through the
        // others.
        //
        // AN ALREADY-OPEN WINDOW IS UNAFFECTED, and that is not an accident of
        // ordering: `get_cert_settlement_deadline` returns the LATER of
        // `expires_at + CHALLENGE_WINDOW` and the freeze this contract wrote
        // when the window opened. While a window is open the freeze is its
        // `closes_at`, which is in the future by construction, so joining
        // claims pass this check and the window runs to completion and settles
        // exactly as it would have.
        //
        // THE COST, STATED PLAINLY. A genuine breach discovered after the
        // deadline is now unchallengeable. That is accepted because bounding
        // precisely this is what the deadline is for — the alternative is
        // collateral that can never safely unwind — and because the alternative
        // reading leaves every honest certificate permanently attackable. It is
        // recorded in DESIGN-V2 §10.
        if now >= Self::cert_settlement_deadline(&env, cert_id) {
            panic!("settlement_deadline_passed");
        }

        // Bond moves into the ChallengeManager and stays at risk until the
        // window closes.
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &challenger,
            &env.current_contract_address(),
            &stake,
        );

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeCount)
            .unwrap();
        let challenge_id = count + 1;

        // ---- DESIGN-V2 §2, mechanism 1: evaluate at filing ----------------
        let (proven, harm, adjudicated) = match proof_type {
            ProofType::InsufficientReserve => {
                let short = Self::reserve_shortfall(&env, cert_id);
                (short > 0, short, true)
            }
            // Both hygiene predicates: true or false is provable now, but the
            // number they produce is a counter and never a loss, so the harm
            // they record is zero. See the comment above `close_window`.
            ProofType::BoundExceeded => (Self::verify_bound_exceeded(&env, cert_id), 0i128, true),
            ProofType::ExpiredCertificate => {
                (Self::verify_expired_certificate(&env, cert_id), 0i128, true)
            }
            // A forged auditor signature leaves no on-chain trace to read, so
            // there is nothing to evaluate at filing. It is recorded
            // un-adjudicated and waits for `resolve_by_arbiter`.
            ProofType::FakeSignature => (false, 0i128, false),
        };

        let ch = Challenge {
            challenger,
            cert_id,
            proof_type,
            victim,
            stake,
            verdict: Verdict::Pending,
            filed_at: now,
            proven,
            harm,
            adjudicated,
            arbitrated: false,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Challenge(challenge_id), &ch);
        env.storage()
            .instance()
            .set(&DataKey::ChallengeCount, &challenge_id);
        env.storage()
            .instance()
            .set(&DataKey::BondsHeld, &(Self::bonds_held(&env) + stake));
        // The challenge record holds the bond liability. Archiving it would
        // abort every later resolve and strand the challenger's bond.
        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Challenge(challenge_id));

        // ---- R3. THE FREEZE IS HELD BY A PROVEN CLAIM, NOT BY THE WINDOW ---
        //
        // The finding: `FakeSignature` has no predicate, so it cannot be
        // rejected at filing the way a false `InsufficientReserve` claim is. It
        // opened a window and froze the certificate — no attestation, no reserve
        // withdrawal, no allocation release — and if the arbiter simply never
        // ruled, the close took the `Unadjudicated` branch and returned the bond
        // IN FULL. DESIGN-V2 §10 priced this griefing surface at "one minimum
        // bond for three days of frozen reserve and allocation"; the actual
        // price was gas plus 72 hours of float, repeatable in 72-hour blocks.
        // `close_window_early`, the mitigation §10 chose, needs the arbiter to
        // actually rule and is therefore unavailable in exactly the scenario
        // that defines the attack.
        //
        // THE FIX: an arbiter-gated claim does not freeze anything. A claim
        // still opens a window, still waits for the arbiter, and still settles
        // normally — it simply does not lock the operator's reserve or the
        // auditor's allocation while it does so. The grief then BUYS NOTHING,
        // which is the only durable answer: there is no longer a thing to
        // purchase, at any price.
        //
        // WHY NOT THE OTHER CANDIDATE — a higher bond for arbiter-gated proof
        // types. It prices the grief instead of removing it, and an attacker who
        // can afford the price still gets the freeze. Worse, on this specific
        // attack the lever does not even connect: the bond is REFUNDED under
        // `Unadjudicated`, so raising it raises the griefer's float and not
        // their cost. Making it bite would mean forfeiting the bond of a
        // claimant whose arbiter never ruled — punishing the innocent for the
        // arbiter's silence — which is the one thing the `Unadjudicated` branch
        // exists to avoid.
        //
        // AND SO `Unadjudicated` STILL REFUNDS IN FULL, now as a decision rather
        // than as a leak. Its stated reason was always right — "a claim nobody
        // judged is not a claim the challenger got wrong" — and it was only ever
        // a problem because the claim bought a freeze on the way past. With the
        // freeze gone the refund costs the protocol nothing.
        //
        // THE FREEZE STILL ATTACHES THE MOMENT ANYTHING BACKS THE CLAIM: a
        // predicate that proved true at filing (below, in both branches, because
        // a proven claim may open a window OR join one an arbiter-gated claim
        // opened), or the arbiter upholding a `FakeSignature` claim
        // (`resolve_by_arbiter`). So the collateral is held whenever there is
        // something to hold it for, and only then.
        //
        // THE COST, STATED PLAINLY. An upheld `FakeSignature` finding can settle
        // against collateral that has already unwound. The exposure is bounded
        // and nameable rather than open-ended: the reserve and the allocation are
        // locked until the certificate's own settlement deadline whatever this
        // contract does, and R1 already refuses any filing from that deadline
        // onwards — so the loss requires the arbiter to still be silent at a
        // deadline the claim was necessarily filed before. That is a latency the
        // arbiter controls, on a party the protocol already trusts completely
        // with the verdict itself. Recorded in DESIGN-V2 §10.
        match open_window {
            Some(mut w) => {
                w.claims.push_back(challenge_id);
                env.storage()
                    .persistent()
                    .set(&DataKey::Window(cert_id), &w);
                Self::bump(&env, &DataKey::Window(cert_id));
                // A proven claim joining a window an arbiter-gated claim opened
                // has to freeze it now — the window itself did not. Re-freezing
                // an already-frozen certificate to the same `closes_at` is
                // idempotent, and `closes_at` never moves once a window is open.
                if proven {
                    Self::freeze(&env, cert_id, w.closes_at);
                }
            }
            None => {
                // Only a claim worth aggregating may open a window. An
                // adjudicated-false claim is settled immediately instead.
                if adjudicated && !proven {
                    Self::reject(&env, challenge_id);
                    return challenge_id;
                }
                let closes_at = now.saturating_add(CLAIM_WINDOW_SECONDS);
                let mut claims = Vec::new(&env);
                claims.push_back(challenge_id);
                env.storage().persistent().set(
                    &DataKey::Window(cert_id),
                    &ClaimWindow {
                        cert_id,
                        opened_at: now,
                        closes_at,
                        claims,
                    },
                );
                Self::bump(&env, &DataKey::Window(cert_id));
                // `proven` here means "a predicate evaluated it true at filing".
                // The only other claim that reaches this line is an
                // arbiter-gated one, which deliberately freezes nothing.
                if proven {
                    Self::freeze(&env, cert_id, closes_at);
                }
            }
        }

        challenge_id
    }

    /// Arbiter adjudication: a verdict and a **quantity** on one live claim.
    /// This is an explicit trust assumption; the arbiter is named at
    /// `initialize`.
    ///
    /// It reaches any claim inside an open window, not just `FakeSignature`.
    /// That is what lets `BoundExceeded` and `ExpiredCertificate` — whose
    /// on-chain predicates are true but whose counters are never a loss — be
    /// given an assessed harm and settle through the full waterfall instead of
    /// hygiene mode.
    ///
    /// WHAT IT CANNOT REACH — **R4**, and this is DESIGN-V2 §2 working in both
    /// directions. On a claim carrying an on-chain predicate the arbiter may
    /// ADD to what the contract proved and may never CONTRADICT it: the verdict
    /// must match what the predicate recorded at filing, and the harm may not
    /// fall below the number it computed. The router and the vault are the
    /// source of truth for what they measure — no human may declare a breach
    /// they say did not happen, and none may deny one that did. The rule is
    /// enforced in the body, where the reasoning is written out in full.
    ///
    /// Only `FakeSignature` is fully the arbiter's to decide, which is the
    /// point: it is the proof type with nothing on-chain to read.
    ///
    /// **This no longer settles anything.** Under the claim window it records a
    /// verdict and a quantity on one claim; the money moves in `close_window`,
    /// together with every other claim in the window and pro rata against the
    /// same collateral. That is what stops an arbiter-gated claim from
    /// foreclosing the arithmetic ones filed beside it.
    ///
    /// The arbiter states the **quantity** as well as the verdict, and that
    /// `harm` feeds the same waterfall every other proof type uses.
    ///
    /// WHY THAT IS SAFE. The arbiter is already a fully trusted party for this
    /// proof type — they are the one deciding whether fraud occurred at all, so
    /// letting them also state the amount grants no new trust. And their number
    /// is bounded by exactly the same rails as an arithmetic one:
    /// `payable = min(total_harm, reserve + allocation)` caps the total
    /// outflow, victim compensation still comes only from the operator's own
    /// reserve for this certificate, and the slash still goes only to the
    /// treasury. An arbiter who overstates harm therefore cannot direct money
    /// to anyone who could have bribed them — the self-dealing property is
    /// preserved by the waterfall, not by the predicate. Overstating harm does
    /// dilute the honest claimants sharing the window, which is a real cost and
    /// is recorded as such in DESIGN-V2 §10.
    pub fn resolve_by_arbiter(env: Env, challenge_id: u64, fraud_proven: bool, harm: i128) {
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();

        if harm < 0 {
            panic!("invalid_harm");
        }
        // A rejected challenge has no harm to quantify. Requiring the zero
        // explicitly, rather than silently ignoring the argument, means a
        // mis-typed call fails loudly instead of recording a verdict that
        // contradicts its own number.
        if !fraud_proven && harm != 0 {
            panic!("harm_without_verdict");
        }

        let mut ch = Self::load_pending(&env, challenge_id);
        // The window is the settlement unit; an adjudication that arrives after
        // it closed cannot be folded into a settlement that already ran.
        let w: ClaimWindow = env
            .storage()
            .persistent()
            .get(&DataKey::Window(ch.cert_id))
            .expect("no_open_window");
        if env.ledger().timestamp() >= w.closes_at {
            panic!("claim_window_closed");
        }

        // ---- R4. The arbiter may ADD to what the contract proved. They may
        // never CONTRADICT it. ------------------------------------------------
        //
        // The finding: nothing here read `ch.proof_type`, so the arbiter could
        // take an `InsufficientReserve` claim the VAULT ITSELF proved true and
        // write `proven = false, harm = 0` over it. That claim is then
        // "rejected", which makes it a valid key to `close_window_early`, so the
        // arbiter could also end the window on the spot. The fraudulent
        // certificate survives reading `Verified`, the auditor is not slashed,
        // and the honest challenger's bond is forfeited for being demonstrably
        // right. R2 widened this: the arbiter is now the ONLY route to victim
        // compensation, so the same party could both withhold compensation and
        // veto the arithmetic that would have slashed.
        //
        // THE RULE, IN ONE SENTENCE. On a claim carrying an on-chain predicate,
        // the arbiter may not move the verdict and may not move the harm
        // downwards. `fraud_proven` must equal what the predicate recorded at
        // filing, and `harm` may not fall below the number the predicate
        // computed. Everything the arbiter is trusted for still works; the one
        // thing they can no longer do is disagree with arithmetic.
        //
        // WHY NOT SIMPLY REFUSE THE ARBITER ON PREDICATE CLAIMS ALTOGETHER,
        // which is the narrower-looking fix. Because that would delete a
        // capability DESIGN-V2 §10 designs for on purpose: `BoundExceeded` and
        // `ExpiredCertificate` settle in HYGIENE MODE precisely because their
        // counters are evidence and never a loss, and the documented way a real
        // loss behind one of them reaches the waterfall is "a human states a
        // number" — `the_arbiter_still_slashes_the_same_breach_when_it_states_harm`
        // is that design, not an oversight. Blocking the arbiter outright would
        // also remove the only correctly-labelled route to compensating a victim
        // of a reserve shortfall, pushing them onto `FakeSignature` to describe
        // facts that are not a forged signature. The monotone rule closes the
        // veto without costing either.
        //
        // WHY THE HARM FLOOR IS PART OF THE SAME RULE AND NOT AN EXTRA. Without
        // it the veto simply changes units: leaving `proven = true` and writing
        // `harm = 1` over a $1,000 shortfall drops the claim out of the
        // predicate group and shrinks the slash to nothing. A quantitative veto
        // is the same veto. Raising the number stays open because the shortfall
        // is a FLOOR on what was lost, not a ceiling — a victim may well have
        // lost more than the operator failed to commit.
        //
        // WHAT THIS ALSO CLOSES, unasked. `resolve_by_arbiter`'s own doc-comment
        // claimed "no human may declare a breach they say did not happen", and
        // that was only true of a claim filed while NO window was open — one
        // that joins an OPEN window is stored `proven = false` rather than
        // rejected on the spot, and the arbiter could flip it to true. Requiring
        // equality refuses that direction too.
        //
        // `FakeSignature` is untouched: it has no predicate to contradict, it is
        // filed `adjudicated = false`, and adjudicating it is the entire reason
        // this entry point exists.
        if ch.proof_type != ProofType::FakeSignature {
            if fraud_proven != ch.proven {
                panic!("predicate_not_overridable");
            }
            if harm < ch.harm {
                panic!("harm_below_predicate");
            }
        }

        ch.proven = fraud_proven;
        ch.harm = harm;
        ch.adjudicated = true;
        ch.arbitrated = true;
        env.storage()
            .persistent()
            .set(&DataKey::Challenge(challenge_id), &ch);
        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Challenge(challenge_id));

        // ---- R3. An upheld claim freezes the certificate from here ----------
        //
        // An arbiter-gated claim no longer freezes anything at filing, so that
        // an ignored one buys the griefer nothing. The instant the arbiter
        // UPHOLDS it, though, there is a finding to settle and the collateral
        // must stay put until the window closes. This is that moment.
        //
        // Idempotent by construction: `closes_at` is fixed when the window opens
        // and a predicate-proven claim in the same window has already written
        // exactly this value. A rejection writes nothing — there is nothing to
        // hold the collateral for, and the window may be closed early anyway.
        if fraud_proven {
            Self::freeze(&env, ch.cert_id, w.closes_at);
        }
    }

    // -----------------------------------------------------------------------
    // DESIGN-V2 §1. Close the window and settle every admitted claim at once.
    //
    // PERMISSIONLESS, AND WHO PAYS FOR IT. Anybody may call this once the
    // window's `closes_at` has passed, and the caller pays the transaction fee.
    // There is deliberately no reward for calling it: a bounty here would be a
    // second pot to game, and there is no need for one, because nobody in the
    // window is paid a stroop until somebody calls it. Every claimant is
    // motivated to be that somebody, and any of them can be. The certificate
    // stays frozen until it happens, so an unclosed window costs the operator
    // and the auditor rather than the victims.
    //
    // WHAT "ADMITTED" MEANS, and it is two different questions:
    //
    //   * Was the challenger RIGHT? Answered from the state RECORDED AT FILING
    //     (`ch.proven`, `ch.harm`). Never recomputed. This is what makes the
    //     bond safe from a mid-window cure.
    //   * Is the certificate still BROKEN? Answered from LIVE state. This is
    //     the cure check, and it can only ever move in the challenger's favour:
    //     a cured claim gets its bond back in full and is simply not paid,
    //     because there is nothing left to compensate.
    //
    // PRO RATA, NOT FIRST-COME. Every pot below is divided by each admitted
    // claim's share of total recorded harm. Nothing reads `claims` order. Two
    // identical claims filed in either order produce byte-identical payouts,
    // which is the property that removes the race rather than relocating it.
    // -----------------------------------------------------------------------
    pub fn close_window(env: Env, cert_id: u64) {
        let w: ClaimWindow = env
            .storage()
            .persistent()
            .get(&DataKey::Window(cert_id))
            .expect("no_open_window");
        if env.ledger().timestamp() < w.closes_at {
            panic!("claim_window_open");
        }
        Self::settle_window(&env, cert_id, &w);
    }

    /// DESIGN-V2 §10. Close a window early, on the arbiter's rejection of the
    /// only thing keeping it open.
    ///
    /// ## The abuse this closes
    ///
    /// `FakeSignature` has **no predicate**. A claim carrying it cannot be
    /// evaluated at filing the way a false `InsufficientReserve` claim is, so
    /// it cannot be rejected on the spot — it opens a window and waits for the
    /// arbiter. It used to freeze the certificate for those 72 hours as well:
    /// no attestation, no reserve withdrawal, no allocation release. Anyone
    /// could buy that freeze against any certificate for the price of the
    /// minimum bond, and an arbiter ruling the claim false did nothing to
    /// shorten it.
    ///
    /// **R3 has since removed the freeze from arbiter-gated claims entirely**,
    /// so the abuse this call was built for no longer has anything to buy. This
    /// call is not thereby redundant: an open window still blocks a second
    /// window on the same certificate and still holds the claimants' bonds, and
    /// ending it on the arbiter's rejection is the right outcome rather than
    /// merely the cheap one.
    ///
    /// The fix is to make the arbiter's rejection *end* the window rather than
    /// merely annotate it. The freeze lifts, the certificate survives, and the
    /// window settles through **exactly the same code path** as a natural
    /// close (`settle_window`) — so the bond is forfeited precisely as it
    /// would have been at `closes_at`. **Getting rejected faster is not
    /// cheaper.** That is the property that stops this from becoming a way to
    /// buy a discount on griefing.
    ///
    /// ## The rail that matters: a live claim always runs the full window
    ///
    /// This may only fire when **no other live claim remains**. A live claim
    /// is any other claim in the window that could still be admitted at close:
    ///
    ///   * one the arbiter has **not yet ruled on** (`!adjudicated`) — the
    ///     arbiter may still uphold it, so it is live by definition; and
    ///   * one that is **proven** — whether an arbiter stated it or a
    ///     predicate computed it.
    ///
    /// A proven claim counts as live even if the operator has since cured the
    /// underlying condition. The cure check belongs at close and only ever at
    /// close: reading it here would let an operator cure mid-window, have a
    /// throwaway claim rejected, and cut the window out from under a genuine
    /// victim who had not yet been paid. The conservative reading is the
    /// correct one — the only claims that do not hold a window open are the
    /// ones already known to be **wrong at filing**, which are precisely the
    /// claims that would be rejected and paid nothing anyway.
    ///
    /// So the arithmetic is: this call closes the window only when settling it
    /// right now is *guaranteed* to admit nothing, which makes the early close
    /// indistinguishable in outcome from the close that was coming anyway. A
    /// cheap arbiter-rejected claim can never cut short a window a genuine
    /// claimant is standing in.
    ///
    /// ## Authority
    ///
    /// Arbiter-only, and only against a claim the arbiter actually ruled on
    /// and ruled **false** (`arbitrated && adjudicated && !proven`). A claim a
    /// *predicate* found false at filing is not a key to this door: nobody
    /// exercised judgement on it, and it was already free to file.
    ///
    /// Kept as its own entry point rather than folded into
    /// `resolve_by_arbiter` so that adjudicating and ending a window stay two
    /// deliberate acts — an arbiter can rule on several claims in a window and
    /// only then decide the window is finished.
    pub fn close_window_early(env: Env, challenge_id: u64) {
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();

        let ch = Self::get(&env, challenge_id);
        if !(ch.arbitrated && ch.adjudicated) {
            panic!("not_arbitrated");
        }
        if ch.proven {
            panic!("claim_not_rejected");
        }

        let w: ClaimWindow = env
            .storage()
            .persistent()
            .get(&DataKey::Window(ch.cert_id))
            .expect("no_open_window");
        if env.ledger().timestamp() >= w.closes_at {
            // Already closable on its own schedule; `close_window` is the call.
            panic!("claim_window_closed");
        }

        for other_id in w.claims.iter() {
            if other_id == challenge_id {
                continue;
            }
            let other = Self::get(&env, other_id);
            if !other.adjudicated || other.proven {
                panic!("live_claim_remains");
            }
        }

        Self::settle_window(&env, ch.cert_id, &w);
    }

    /// Everything a close does once it has been decided that it may happen.
    /// Shared by `close_window` and `close_window_early` so the two cannot
    /// drift: an early close forfeits and returns exactly the same bonds, in
    /// exactly the same way, as the close that was coming anyway.
    fn settle_window(env: &Env, cert_id: u64, w: &ClaimWindow) {
        let env = env.clone();
        let w = w.clone();
        // Pass 1 — classify, and settle everything that is not a payout.
        // Rejections run before the admitted set is priced so that the hygiene
        // bounty sees a pool that does not depend on filing order.
        let mut admitted: Vec<u64> = Vec::new(&env);
        // How much of the harm each admitted claim is entitled to a share of.
        // Not the same as `ch.harm` — see the aggregation rule below.
        let mut weights: Vec<i128> = Vec::new(&env);
        // The certificate-level shortfall, and how many claims are standing on
        // it. `InsufficientReserve` is the one predicate that produces a
        // quantity, and that quantity belongs to the CERTIFICATE, not to the
        // claimant who noticed it.
        let mut shortfall: i128 = 0;
        let mut shortfall_claims: i128 = 0;
        for challenge_id in w.claims.iter() {
            let ch = Self::get(&env, challenge_id);
            if !ch.adjudicated {
                // The arbiter never ruled. Not the challenger's fault, so the
                // bond comes back whole.
                Self::return_bond(&env, &ch);
                Self::record(&env, challenge_id, &ch, Verdict::Unadjudicated);
            } else if !ch.proven {
                // Wrong at filing. The only outcome that forfeits a bond, and a
                // later cure cannot launder it into anything else.
                Self::reject(&env, challenge_id);
            } else if !Self::live_predicate(&env, &ch) {
                // DESIGN-V2 §2, mechanism 2: right at filing, fixed since.
                Self::return_bond(&env, &ch);
                Self::record(&env, challenge_id, &ch, Verdict::Cured);
            } else {
                admitted.push_back(challenge_id);
                weights.push_back(0);
                if !ch.arbitrated && ch.proof_type == ProofType::InsufficientReserve {
                    shortfall_claims += 1;
                    // The WORST state the certificate was in at any filing
                    // during the window. Max rather than sum, and this is the
                    // load-bearing choice — see the rule below.
                    if ch.harm > shortfall {
                        shortfall = ch.harm;
                    }
                }
            }
        }

        // ---- HOW HARM AGGREGATES ACROSS A WINDOW --------------------------
        //
        // Two kinds of number reach this point and they must not be added up
        // the same way.
        //
        //   * A harm the ARBITER STATED is an assessment of what ONE claimant
        //     lost. Two claimants who each lost $500 lost $1,000 between them,
        //     so these SUM.
        //
        //   * A harm a PREDICATE COMPUTED is a property of the CERTIFICATE.
        //     `InsufficientReserve` reads one shortfall off one vault; ten
        //     people noticing the same $800 hole have not proven $8,000 of
        //     harm. So the shortfall is counted ONCE and SHARED EQUALLY by the
        //     claims standing on it.
        //
        // ATTACK CLOSED: harm amplification. If identical predicate claims
        // summed, anyone could file n copies of the same true proof and drive
        // `payable` — and with it the auditor's slash — to n times the real
        // shortfall, up to the whole allocation, for n minimum bonds. The
        // waterfall's "capped by proven harm" rail would be capped by proven
        // harm times a number the attacker picks.
        //
        // Equal shares rather than pro rata within the predicate group because
        // the predicate cannot tell the claimants apart: it reads the vault,
        // not the victims. Equality is also the only order-independent answer.
        let mut total_harm: i128 = 0;
        let mut i: u32 = 0;
        while i < admitted.len() {
            let ch = Self::get(&env, admitted.get(i).unwrap());
            let weight = if ch.arbitrated {
                ch.harm
            } else if ch.proof_type == ProofType::InsufficientReserve {
                shortfall / shortfall_claims
            } else {
                // Both hygiene predicates. True, but a counter is never a loss.
                0
            };
            weights.set(i, weight);
            total_harm += weight;
            i += 1;
        }

        env.storage().persistent().remove(&DataKey::Window(cert_id));

        if admitted.is_empty() {
            // Nothing survived. Either every claim was cured, or none was ever
            // upheld. The certificate lives, and the freeze lifts so the
            // operator's reserve and the auditor's allocation can unwind on
            // their own schedule again. A fresh window may open later.
            Self::freeze(&env, cert_id, 0);
            return;
        }

        env.storage()
            .persistent()
            .set(&DataKey::Settled(cert_id), &true);
        // `Settled` is what refuses a second window on a dead certificate. If it
        // archived, the certificate would become challengeable all over again.
        Self::bump(&env, &DataKey::Settled(cert_id));
        if total_harm > 0 {
            Self::settle_fraud(&env, cert_id, &admitted, &weights, total_harm);
        } else {
            Self::settle_hygiene(&env, cert_id, &admitted);
        }
        // The certificate is dead; there is nothing left for a freeze to
        // protect, and `Settled` refuses any further window on it.
        Self::freeze(&env, cert_id, 0);
    }

    /// The claim window, if one is open on this certificate.
    pub fn get_window(env: Env, cert_id: u64) -> Option<ClaimWindow> {
        env.storage().persistent().get(&DataKey::Window(cert_id))
    }

    /// Ledger time at which this certificate's open window may be closed.
    /// `0` means no window is open.
    pub fn window_closes_at(env: Env, cert_id: u64) -> u64 {
        let w: Option<ClaimWindow> = env.storage().persistent().get(&DataKey::Window(cert_id));
        match w {
            None => 0,
            Some(w) => w.closes_at,
        }
    }

    /// Whether this certificate has already been settled through a closed
    /// window. A settled certificate accepts no further claims.
    pub fn is_settled(env: Env, cert_id: u64) -> bool {
        env.storage().persistent().has(&DataKey::Settled(cert_id))
    }

    /// How long a claim window stays open, in ledger seconds.
    pub fn get_claim_window_seconds(env: Env) -> u64 {
        let _ = env;
        CLAIM_WINDOW_SECONDS
    }

    pub fn get_challenge(env: Env, challenge_id: u64) -> Challenge {
        env.storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found")
    }

    pub fn get_treasury(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Treasury).unwrap()
    }

    /// Forfeited bonds the contract holds beyond what it still owes back. This
    /// is the only pot the hygiene bounty is paid from.
    pub fn get_bounty_pool(env: Env) -> i128 {
        Self::bounty_pool(&env)
    }

    pub fn get_bonds_held(env: Env) -> i128 {
        Self::bonds_held(&env)
    }

    pub fn get_challenge_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::ChallengeCount)
            .unwrap_or(0)
    }

    // ----- internals -----

    /// Defect L2. Bump the instance entry (which carries the contract's own
    /// code reference) so the contract cannot archive out from under its users.
    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Defect L2. Bump one persistent entry.
    fn bump(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    fn get(env: &Env, challenge_id: u64) -> Challenge {
        env.storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found")
    }

    fn load_pending(env: &Env, challenge_id: u64) -> Challenge {
        let ch = Self::get(env, challenge_id);
        if ch.verdict != Verdict::Pending {
            panic!("already_resolved");
        }
        ch
    }

    /// Open or lift the certificate's freeze, through the Registry.
    ///
    /// DESIGN-V2 §1 asks for the certificate to be frozen while a window is
    /// open — no new attestation, no reserve withdrawal, no allocation release,
    /// and no escape through expiry. Rather than invent a second lock, the
    /// freeze is written as an extension of the **settlement deadline** the
    /// Registry already publishes and that both money contracts already refuse
    /// to release before. One mechanism, three enforcement points, no way for
    /// them to drift apart.
    fn freeze(env: &Env, cert_id: u64, until: u64) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        env.invoke_contract::<()>(
            &registry,
            &Symbol::new(env, "set_claim_freeze"),
            Vec::from_array(env, [cert_id.into_val(env), until.into_val(env)]),
        );
    }

    /// Is the certificate STILL broken, judged on live state?
    ///
    /// This is the cure check and nothing else. It never decides whether the
    /// challenger was right — `ch.proven`, recorded at filing, already did that
    /// and is never revisited. A `false` here can only ever help the
    /// challenger: it returns their bond in full and spares the certificate.
    ///
    /// `FakeSignature` has no live predicate to re-read. A forged signature
    /// cannot be un-forged, so an arbiter's verdict is not curable and the
    /// recorded value stands.
    fn live_predicate(env: &Env, ch: &Challenge) -> bool {
        match ch.proof_type {
            ProofType::InsufficientReserve => Self::reserve_shortfall(env, ch.cert_id) > 0,
            ProofType::BoundExceeded => Self::verify_bound_exceeded(env, ch.cert_id),
            ProofType::ExpiredCertificate => Self::verify_expired_certificate(env, ch.cert_id),
            ProofType::FakeSignature => ch.proven,
        }
    }

    /// Wrong at filing: the bond is forfeited. It stays in the contract and
    /// stops being owed back, which is what makes it available as hygiene
    /// bounty for a future, genuine challenge.
    fn reject(env: &Env, challenge_id: u64) {
        let ch = Self::get(env, challenge_id);
        Self::release_bond_liability(env, ch.stake);
        Self::record(env, challenge_id, &ch, Verdict::ChallengeFails);
    }

    /// On-chain proof, and its quantity in one read: how far this
    /// certificate's reserve falls short of what it claims.
    ///
    /// Zero means no shortfall, so `> 0` is the predicate and the value itself
    /// is the harm. Keeping the two together means the predicate and the number
    /// it sizes can never be evaluated at two different ledgers, which is
    /// precisely the drift DESIGN-V2 §2 is about.
    ///
    /// The named victim is deliberately NOT part of this number. A named victim
    /// is a filter, not a proof: receiving a payment is evidence of being paid,
    /// not of being harmed. The waterfall neutralises a self-named victim
    /// rather than the predicate trying to authenticate one.
    fn reserve_shortfall(env: &Env, cert_id: u64) -> i128 {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        let reserve_vault: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReserveVault)
            .unwrap();

        let claimed: i128 = env.invoke_contract(
            &registry,
            &Symbol::new(env, "get_cert_reserve"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );
        // Per-certificate accounting: the proof compares the certificate's own
        // claim against the certificate's own reserve. A deposit made for any
        // other certificate is invisible here.
        let actual: i128 = env.invoke_contract(
            &reserve_vault,
            &Symbol::new(env, "get_balance"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );

        if claimed > actual {
            claimed - actual
        } else {
            0
        }
    }

    /// On-chain proof: the router has metered more gross flow against this
    /// certificate than the certificate's own bound allows.
    ///
    /// The router is the source of truth, and it is a sound one: only an
    /// enrolled agent moves the counter, enrollment needs both the agent's and
    /// the certificate operator's signature, and an enrollment is permanent. So
    /// nobody can attach spend to a stranger's certificate, and an operator
    /// cannot walk an agent off a climbing counter onto a fresh certificate.
    ///
    /// An agent that never enrolled meters nothing, so `spent` stays 0 and this
    /// predicate is false — untracked agents cannot be proven against here.
    ///
    /// The arithmetic is unforgeable and the number is still not a loss. See the
    /// hygiene-mode comment on `resolve`.
    fn verify_bound_exceeded(env: &Env, cert_id: u64) -> bool {
        let bound = Self::cert_bound(env, cert_id);
        Self::router_spent(env, cert_id) > bound
    }

    /// On-chain proof of DESIGN-V2 §7: the router recorded a payment that
    /// settled late enough, and was large enough, to be evidence that the
    /// certificate's covenant outlived the certificate.
    ///
    /// All three §7 conditions must hold:
    ///
    /// 1. The payment settled after `expires_at + GRACE_WINDOW_SECONDS`.
    /// 2. Its value was at least `DE_MINIMIS_FLOOR_BPS` of the certificate's
    ///    bound.
    /// 3. The certificate was not renewed, and is not already invalid.
    ///
    /// **Which payment.** The router records exactly one post-expiry pair for
    /// this purpose: `max_payment` and `max_payment_at`, the largest late
    /// payment and when it settled. Conditions 1 and 2 are applied to that one
    /// pair. That is deliberately conservative in one direction: if the largest
    /// late payment landed inside the grace window while a smaller, later one
    /// cleared both tests, this returns false. Upholding fewer real breaches is
    /// the safe error here — the cost of a false negative is a certificate that
    /// lives out its term, and the cost of a false positive is an honest
    /// certificate killed. Recording a per-payment history to close the gap
    /// would put unbounded storage on the router's x402 hot path.
    ///
    /// **Renewal, condition 3.** This registry has no in-place extension —
    /// `expires_at` is immutable once published — so renewing means publishing a
    /// fresh certificate for the same agent, which re-points the agent's
    /// mapping. If the agent's current certificate is no longer this one, the
    /// covenant moved and this certificate's late activity is not evidence
    /// against it. Note the timing is coarser than §7's wording: §7 says "not
    /// renewed **before** the payment", and this asks "not renewed as of
    /// resolution". A renewal filed after the late payment therefore also
    /// defeats the proof, which is a cure path in the sense of §2 — and §2's
    /// evaluate-at-filing machinery now exists, so such a claim resolves as
    /// `Cured` and the challenger's bond comes back whole rather than being
    /// forfeited for a renewal they could not have foreseen. The coarseness
    /// itself remains, errs toward not killing a certificate, and is recorded
    /// as an open gap in `docs/DESIGN-V2.md` §10 rather than silently
    /// accepted.
    ///
    /// An agent that never enrolled has no post-expiry record at all, so the
    /// predicate is false for it.
    fn verify_expired_certificate(env: &Env, cert_id: u64) -> bool {
        // Condition 3, first, because it is the cheapest and the most decisive.
        // Already-invalid certificates are excluded too: there is nothing left
        // to kill, and a second hygiene bounty for the same dead certificate
        // would be a slow drain on the forfeited-bond pool.
        if !Self::cert_is_verified(env, cert_id) {
            return false;
        }
        let agent = Self::cert_agent(env, cert_id);
        if Self::current_cert_of_agent(env, &agent) != cert_id {
            return false;
        }

        let pe = Self::router_post_expiry(env, cert_id);
        if pe.count == 0 {
            return false;
        }

        // Condition 1: after expiry PLUS the grace window. `expires_at` is read
        // live from the Registry rather than from the router's enrollment-time
        // snapshot, so the certificate the challenge names is the one the window
        // is measured against.
        let expires_at = Self::cert_expires_at(env, cert_id);
        let deadline = expires_at.saturating_add(GRACE_WINDOW_SECONDS);
        if pe.max_payment_at <= deadline {
            return false;
        }

        // Condition 2: at least 0.1% of THIS certificate's bound.
        let floor = Self::cert_bound(env, cert_id) * DE_MINIMIS_FLOOR_BPS / 10_000;
        pe.max_payment >= floor
    }

    // ----- reads on the other contracts ----------------------------------

    fn router(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Router)
            .expect("router_not_set")
    }

    /// The PremiumVault, if one was ever wired.
    ///
    /// Optional rather than `expect`ing, unlike `router()`. A deployment that
    /// predates the premium economy — or one whose deploy script skipped
    /// `set_premium_vault` — must still settle, and skipping a step that has no
    /// money behind it is correct. THE TRAP IS THE SAME ONE `set_router` HAS:
    /// a deployment that forgets the wiring looks completely healthy and
    /// silently settles every challenge without step 4. `has_premium_vault()`
    /// exists so a deploy check can catch it.
    fn premium_vault(env: &Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PremiumVault)
    }

    fn registry(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::Registry).unwrap()
    }

    fn router_spent(env: &Env, cert_id: u64) -> i128 {
        env.invoke_contract(
            &Self::router(env),
            &Symbol::new(env, "spent"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn router_post_expiry(env: &Env, cert_id: u64) -> PostExpiry {
        env.invoke_contract(
            &Self::router(env),
            &Symbol::new(env, "post_expiry_spent"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn cert_bound(env: &Env, cert_id: u64) -> i128 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_bound"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn cert_expires_at(env: &Env, cert_id: u64) -> u64 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_expires_at"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    /// The instant this certificate's collateral may unwind, read live so that
    /// an open claim window's extension is included. See the R1 rail in
    /// `challenge`.
    fn cert_settlement_deadline(env: &Env, cert_id: u64) -> u64 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_settlement_deadline"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn cert_agent(env: &Env, cert_id: u64) -> Address {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_agent"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn cert_is_verified(env: &Env, cert_id: u64) -> bool {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "is_cert_verified"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn current_cert_of_agent(env: &Env, agent: &Address) -> u64 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_id"),
            Vec::from_array(env, [agent.clone().into_val(env)]),
        )
    }

    // ----- the settlement waterfall -------------------------------------
    //
    // ONE rule, applied identically to every proof type:
    //
    //     harm    = raw harm from the predicate, or stated by the arbiter
    //     payable = min(harm, reserve_for_this_cert + allocation_for_this_cert)
    //
    //     1. victim compensation  <- the operator's OWN reserve for THIS cert
    //     2. challenger fee       <- the same reserve, % of PROVEN HARM
    //     3. auditor slash        -> the TREASURY, never victim or challenger
    //     4. forfeited premium  -> the victim, capped by uncovered harm;
    //                                the unaccrued remainder -> the TREASURY
    //     5. allocation retires; unslashed remainder returns to free stake
    //     6. certificate invalidated; challenger's bond returned
    //
    // Every line closes a specific attack. Before "simplifying" any of them,
    // read the reason attached to it — v1 had none of these and paid a
    // colluding operator the auditor's entire bond for the price of one lie.

    /// Settle every admitted claim in one pass, pro rata against the
    /// certificate's collateral.
    ///
    /// `total_harm` is the sum of the admitted claims' **recorded** harm. Each
    /// pot below is divided by each claim's share of it. Nothing reads the
    /// order the claims were filed in.
    fn settle_fraud(
        env: &Env,
        cert_id: u64,
        admitted: &Vec<u64>,
        weights: &Vec<i128>,
        total_harm: i128,
    ) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        let staking: Address = env
            .storage()
            .instance()
            .get(&DataKey::AuditorStaking)
            .unwrap();
        let vault: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReserveVault)
            .unwrap();
        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();

        // The two pots this certificate can be settled against, and nothing
        // else. Both are keyed by cert_id: no other certificate's reserve and no
        // other certificate's allocation is reachable from here.
        let reserve: i128 = env.invoke_contract(
            &vault,
            &Symbol::new(env, "get_balance"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );
        let allocation: i128 = env.invoke_contract(
            &staking,
            &Symbol::new(env, "get_allocation"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );

        // The ceiling on everything below. Total outflow can never exceed the
        // harm actually proven, nor the collateral this certificate carries.
        let payable = if total_harm < reserve + allocation {
            total_harm
        } else {
            reserve + allocation
        };

        // ---- WHO MAY BE PAID COMPENSATION -------------------------------
        //
        // R2. Only an ARBITER-ASSESSED claim names a victim the protocol has
        // any reason to believe in. A predicate-computed proof establishes that
        // THE COVENANT WAS BROKEN, not that any particular person was harmed —
        // that is the protocol's own founding principle, and it is exactly why
        // `BoundExceeded` and `ExpiredCertificate` settle in hygiene mode with
        // no victim payment at all. `InsufficientReserve` was treated
        // differently only by inheritance from v1, and the consequence was a
        // High finding:
        //
        //   the certificate-level shortfall is counted once and split equally
        //   between the claims standing on it, each claim pays the address it
        //   named, and an admitted claim gets its bond back in full. So `n`
        //   extra TRUE claims from throwaway addresses cost gas plus 72 hours
        //   of bond float and take `n/(n+1)` of the victim pool from the honest
        //   victim. The contract was trying to compensate victims it has no way
        //   to identify, so whoever showed up in the largest numbers took the
        //   pot.
        //
        // ADDRESS DE-DUPLICATION IS NOT A FIX and was rejected: a sybil farms
        // addresses, and dedupe only raises the price. Nor is a non-refundable
        // filing fee (same — it prices the attack rather than removing it) or a
        // cap on claims per window (the attacker fills the cap and forecloses
        // the honest claimant outright). The dilution is not a bug in the
        // splitting rule; it is the splitting rule being asked a question the
        // predicate cannot answer.
        //
        // So the predicate group is paid NO victim compensation. Its share of
        // the reserve draw goes to the TREASURY, which is not a party to the
        // window and cannot be sybilled into being one. Every other number
        // below — the size of the reserve draw, `reserve_left`, the fee pool,
        // the slash — is unchanged, so the operator still pays first and the
        // auditor still covers only the remainder. All that changes is the
        // recipient of the predicate share.
        //
        // THE COST, STATED PLAINLY. The protocol's only trustless proof no
        // longer compensates anyone directly. A victim's compensation now
        // always requires the arbiter to name and size them. That narrows the
        // trustless surface and it contradicts earlier framing; it is recorded
        // in DESIGN-V2 §1 and §10 rather than hidden.
        // The compensable claims and their total. `arbitrated_harm` — not
        // `total_harm` — is the denominator every victim payout below divides
        // by, and that is load-bearing: dividing by `total_harm` would let a
        // single free predicate claim shave the assessed victim's share and
        // send the difference to the treasury. It also makes the payout
        // independent of how many predicate claims stand beside it, which is
        // the property R2 needs.
        let mut victim_weights: Vec<i128> = Vec::new(env);
        let mut arbitrated_harm: i128 = 0;
        let mut vi: u32 = 0;
        while vi < admitted.len() {
            let ch = Self::get(env, admitted.get(vi).unwrap());
            let w = if ch.arbitrated {
                weights.get(vi).unwrap()
            } else {
                0
            };
            victim_weights.push_back(w);
            arbitrated_harm += w;
            vi += 1;
        }

        // 1. Victim compensation — from the OPERATOR'S OWN RESERVE for THIS
        //    certificate only, split pro rata across every admitted claim that
        //    an arbiter assessed.
        //
        //    ATTACK CLOSED: self-dealing. A colluding operator that names an
        //    address it controls as "victim" is moving money from its left
        //    pocket to its right. Manufacturing a proof against yourself
        //    extracts nothing, so the permissiveness of victim naming stops
        //    mattering.
        //
        //    ATTACK CLOSED: foreclosure. Under the old lifecycle this pot was
        //    handed whole to whoever settled first. It is now shared by
        //    everyone the window admitted, in proportion to what they proved.
        //
        //    ATTACK CLOSED: sybil dilution. See the note above — the shares of
        //    the predicate group are zero here and fall to the treasury.
        //
        //    `reserve_draw` is the whole of step 1 and is sized exactly as it
        //    always was, so `reserve_left`, `residual` and the slash below are
        //    untouched by R2. `victim_pool` is the part of it that has somebody
        //    to go to; the rest falls to the treasury.
        let reserve_draw = if payable < reserve { payable } else { reserve };
        let victim_pool = if reserve_draw < arbitrated_harm {
            reserve_draw
        } else {
            arbitrated_harm
        };
        let victim_paid = Self::distribute(
            env,
            admitted,
            &victim_weights,
            victim_pool,
            arbitrated_harm,
            &|ch: &Challenge, amount: i128| {
                Self::pay_from_reserve(env, &vault, cert_id, &ch.victim, amount)
            },
        );
        // What step 1 did not hand to a victim. Two things land here now: the
        // rounding remainder, because integer division truncates and the shares
        // can sum to a stroop or two under the pool; and — since R2 — the whole
        // share of every predicate-computed claim, which is a real amount and
        // not a rounding crumb. Both go to the TREASURY rather
        // than to a claimant, because handing it to "the largest claim" or "the
        // first claim" would make a payout depend on tie-breaking — which is
        // ordering value, the exact thing this window exists to destroy. The
        // treasury is not a party to the window and cannot be gamed into being
        // one. The amount is bounded by (claims - 1) stroops, i.e. 10^-7 USDC
        // each. Nothing is stranded and nothing is conjured: the pots below are
        // sized so that the total leaving the reserve is exactly `victim_pool`.
        if reserve_draw - victim_paid > 0 {
            Self::pay_from_reserve(env, &vault, cert_id, &treasury, reserve_draw - victim_paid);
        }
        let reserve_left = reserve - reserve_draw;

        // 2. Challenger fees — a percentage of PROVEN HARM, out of the same
        //    reserve, never out of the stake, split pro rata by the harm each
        //    claim proved.
        //
        //    ATTACK CLOSED: bounty-hunting auditors. v1 paid 20% of the
        //    auditor's live stake, so the most profitable target was whoever
        //    had posted the most collateral, regardless of how small the fraud
        //    was.
        //
        //    NOT CLOSED, AND DELIBERATELY SO: sybil claims still split this
        //    pool. R2's fix removes them from the victim pot, not from here,
        //    and that is the right line. This is a BOUNTY for surfacing a fact,
        //    not compensation for a loss. Its total is fixed at 10% of proven
        //    harm however many people file, so a sybil cannot enlarge it — they
        //    can only take a share of one honest challenger's bounty, which is
        //    the same trade the hygiene bounty already makes on purpose ("ONE
        //    flat bounty for the window, split EQUALLY, not one bounty each").
        //    Removing it would need an identity the predicate does not have.
        //    Recorded in DESIGN-V2 §10.
        let mut fee_pool = total_harm * CHALLENGER_FEE_BPS / 10_000;
        if fee_pool > reserve_left {
            fee_pool = reserve_left;
        }
        let fee_paid = Self::distribute(
            env,
            admitted,
            weights,
            fee_pool,
            total_harm,
            &|ch: &Challenge, amount: i128| {
                Self::pay_from_reserve(env, &vault, cert_id, &ch.challenger, amount)
            },
        );
        if fee_pool - fee_paid > 0 {
            Self::pay_from_reserve(env, &vault, cert_id, &treasury, fee_pool - fee_paid);
        }

        // 3. Auditor slash — to the TREASURY, and only the treasury.
        //
        //    ATTACK CLOSED: the prize. Nobody who can trigger a proof can
        //    receive the auditor's money, so manufacturing a true proof pays
        //    nothing however true it is.
        //
        //    The draw is the harm the operator's own reserve could not cover,
        //    capped by this certificate's allocation.
        //
        //    ATTACK CLOSED: disproportion. A manufactured $10 breach cannot cost
        //    an auditor a $50,000 bond — it is capped by harm — and one bad
        //    certificate cannot destroy an auditor's whole book, because it is
        //    capped by that certificate's allocation. Aggregating a window does
        //    not widen either cap: it is one slash against one allocation,
        //    sized by the harm of the whole window rather than of one claim.
        let residual = payable - reserve_draw;
        let slash = if residual < allocation {
            residual
        } else {
            allocation
        };
        if slash > 0 {
            env.invoke_contract::<()>(
                &staking,
                &Symbol::new(env, "slash_allocation"),
                Vec::from_array(
                    env,
                    [
                        cert_id.into_val(env),
                        treasury.clone().into_val(env),
                        slash.into_val(env),
                    ],
                ),
            );
        }

        // 4. Forfeited premium. The auditor vouched for a certificate that has
        //    now been proven harmful, so they forfeit the yield they have not
        //    already withdrawn. `PremiumVault::forfeit` splits it: accrued-but-
        //    unclaimed to the named recipient, capped below; everything else —
        //    the excess over the cap plus the entire unaccrued remainder — to
        //    the treasury.
        //
        //    THE RECIPIENT IS THIS CONTRACT, not a victim, and that is the only
        //    change the claim window forces on step 4. A window can admit many
        //    victims and `forfeit` pays one address; calling it once per victim
        //    would hand the whole pot to whoever was called first, which is the
        //    ordering value this window exists to remove. So the ChallengeManager
        //    takes delivery and fans the money out pro rata in the same call.
        //    It is a conduit for the length of one invocation: nothing observes
        //    the intermediate balance, and `bonds_held` is untouched by it, so
        //    the bounty pool is unaffected.
        //
        //    CONSISTENT WITH RULE 1, and this is the whole reason it is allowed
        //    to reach a victim at all: **the premium is the operator's own
        //    money.** Every stroop in that pot was paid in by this certificate's
        //    operator and has not yet been handed to anyone. Paying a victim out
        //    of it is the same left-pocket-to-right move that makes a
        //    self-dealing operator's "compensation" a wash.
        //
        //    CONSISTENT WITH RULE 3: nothing here can move the auditor's stake.
        //    The vault has no reference to AuditorStaking and moves only tokens
        //    it already holds.
        //
        //    THE CAP is `arbitrated_harm - victim_pool`: the ASSESSED harm the
        //    operator's own reserve did not already cover. Without it, a large
        //    premium could pay victims more than the harm proven against the
        //    certificate, breaking the waterfall's "capped by proven harm"
        //    invariant. It is `arbitrated_harm` and not `total_harm` for R2's
        //    reason: a predicate proof evidences no victim, so it must not
        //    enlarge the pot the named victims share. When no arbiter assessed
        //    anything the cap is zero and the whole forfeited premium goes to
        //    the treasury, exactly as in hygiene mode.
        //
        //    WHY `payable` IS NOT RAISED BY THE FORFEITED PREMIUM. It was
        //    considered and rejected, because it is arithmetically a no-op and a
        //    readability loss. `payable` only ever binds through two mins:
        //    `victim_pool = min(payable, reserve)` and
        //    `slash = min(payable - victim_pool, allocation)`. Adding a
        //    premium `P` gives `payable' = min(harm, reserve + allocation + P)`.
        //    If `harm < reserve + allocation` nothing changes at all. Otherwise
        //    `victim_pool` is `reserve` either way, and `slash` is capped at
        //    `allocation` either way.
        if let Some(pv) = Self::premium_vault(env) {
            let victim_cap = arbitrated_harm - victim_pool;
            let received = env.invoke_contract::<i128>(
                &pv,
                &Symbol::new(env, "forfeit"),
                Vec::from_array(
                    env,
                    [
                        cert_id.into_val(env),
                        env.current_contract_address().into_val(env),
                        victim_cap.into_val(env),
                    ],
                ),
            );
            if received > 0 {
                let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
                let client = token::Client::new(env, &token_addr);
                let me = env.current_contract_address();
                // R2 again: this pot pays `ch.victim`, so it is sybil-diluted
                // by exactly the same mechanism as step 1 and is restricted to
                // exactly the same claims. The predicate group's share falls to
                // the treasury on the line below.
                let handed = Self::distribute(
                    env,
                    admitted,
                    &victim_weights,
                    received,
                    arbitrated_harm,
                    &|ch: &Challenge, amount: i128| client.transfer(&me, &ch.victim, &amount),
                );
                // Same rounding rule as step 1, and it matters more here: any
                // stroop left behind would silently join the bounty pool, which
                // is forfeited-bond money and nothing else.
                if received - handed > 0 {
                    client.transfer(&me, &treasury, &(received - handed));
                }
            }
        }

        // 5. Retire the allocation: the unslashed remainder goes back to the
        //    auditor's FREE stake.
        //
        //    ATTACK CLOSED (against the auditor, by omission): stranded capital.
        //    Without this the remainder would stay allocated to a dead
        //    certificate forever — which is exactly the defect the
        //    per-certificate refactor exists to remove, quietly reintroduced.
        env.invoke_contract::<()>(
            &staking,
            &Symbol::new(env, "retire_allocation"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );

        // 6. Kill the certificate and return every admitted challenger's bond.
        Self::invalidate(env, &registry, cert_id);
        for challenge_id in admitted.iter() {
            let ch = Self::get(env, challenge_id);
            Self::return_bond(env, &ch);
            Self::record(env, challenge_id, &ch, Verdict::ChallengeWins);
        }
    }

    /// Hand out `pool` in proportion to each admitted claim's recorded harm,
    /// and report the total actually handed out.
    ///
    /// `pool * harm / total_harm`, truncating. The sum of the truncated shares
    /// is at most `pool`, never more, so this can never conjure a stroop; the
    /// caller routes the shortfall to the treasury so it can never strand one
    /// either. A claim whose harm rounds to zero is paid nothing, which is
    /// correct — it proved a share of the loss too small to be expressible.
    ///
    /// The closure is the only thing that differs between the three pots, and
    /// none of them may depend on iteration order: `pool` and `total_harm` are
    /// both fixed before the first payment.
    fn distribute(
        env: &Env,
        admitted: &Vec<u64>,
        weights: &Vec<i128>,
        pool: i128,
        total_harm: i128,
        pay: &dyn Fn(&Challenge, i128),
    ) -> i128 {
        if pool <= 0 || total_harm <= 0 {
            return 0;
        }
        let mut paid: i128 = 0;
        let mut i: u32 = 0;
        while i < admitted.len() {
            let ch = Self::get(env, admitted.get(i).unwrap());
            let share = pool * weights.get(i).unwrap() / total_harm;
            if share > 0 {
                pay(&ch, share);
                paid += share;
            }
            i += 1;
        }
        paid
    }

    /// Hygiene mode: every admitted claim is real, but none of them evidences
    /// harm.
    ///
    /// The certificate is invalidated, the allocation retires in full, the
    /// challengers split a FIXED bounty out of forfeited bonds, and the
    /// operator's reserve is not touched at all. A dead certificate dies without
    /// a payout being invented for it.
    fn settle_hygiene(env: &Env, cert_id: u64, admitted: &Vec<u64>) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        let staking: Address = env
            .storage()
            .instance()
            .get(&DataKey::AuditorStaking)
            .unwrap();

        env.invoke_contract::<()>(
            &staking,
            &Symbol::new(env, "retire_allocation"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );

        // Step 4 in hygiene mode. The auditor was not slashed and nobody is
        // evidenced as harmed, so nothing is forfeited to a victim: accrual
        // stops at the kill, the auditor keeps — and can still claim — the
        // share they earned before it, and the unaccrued remainder goes to the
        // treasury.
        //
        // The remainder is deliberately NOT refunded to the operator. DESIGN-V2
        // §10 already prices a hygiene kill as costing the operator "their own
        // certificate, their reserve lockup and their premium": both hygiene
        // predicates are manufacturable by the operator at the price of gas, and
        // a refund would make manufacturing one free.
        if let Some(pv) = Self::premium_vault(env) {
            env.invoke_contract::<i128>(
                &pv,
                &Symbol::new(env, "terminate"),
                Vec::from_array(env, [cert_id.into_val(env)]),
            );
        }

        Self::invalidate(env, &registry, cert_id);

        // Paid from forfeited bonds only — failed challengers fund successful
        // hygiene challenges. There is no other pot it could come from: the
        // reserve is off limits by definition here, and paying it out of the
        // stake would hand a challenger a slice of the auditor's money, which is
        // the thing rule 3 exists to forbid. If the pool is short, the bounty is
        // whatever the pool holds, including nothing.
        //
        // ONE flat bounty for the window, split EQUALLY, not one bounty each.
        // Its job is to pay for the gas of killing a dead certificate, and that
        // job is done once however many people showed up to do it — otherwise
        // filing n copies of the same hygiene proof would mint n bounties.
        // Equal shares rather than pro rata because hygiene harm is zero by
        // definition, so there is no ratio to divide by; equal shares are
        // order-independent, which is what actually matters here. The remainder
        // of an unequal division stays in the pool.
        let pool = Self::bounty_pool(env);
        let bounty = if HYGIENE_BOUNTY < pool {
            HYGIENE_BOUNTY
        } else {
            pool
        };
        let share = bounty / (admitted.len() as i128);
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(env, &token_addr);
        for challenge_id in admitted.iter() {
            let ch = Self::get(env, challenge_id);
            Self::return_bond(env, &ch);
            if share > 0 {
                client.transfer(&env.current_contract_address(), &ch.challenger, &share);
            }
            Self::record(env, challenge_id, &ch, Verdict::ChallengeWins);
        }
    }

    // ----- small helpers -------------------------------------------------

    fn pay_from_reserve(env: &Env, vault: &Address, cert_id: u64, to: &Address, amount: i128) {
        env.invoke_contract::<()>(
            vault,
            &Symbol::new(env, "pay_from_reserve"),
            Vec::from_array(
                env,
                [
                    cert_id.into_val(env),
                    to.clone().into_val(env),
                    amount.into_val(env),
                ],
            ),
        );
    }

    fn invalidate(env: &Env, registry: &Address, cert_id: u64) {
        env.invoke_contract::<()>(
            registry,
            &Symbol::new(env, "invalidate"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );
    }

    fn return_bond(env: &Env, ch: &Challenge) {
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(env, &token_addr).transfer(
            &env.current_contract_address(),
            &ch.challenger,
            &ch.stake,
        );
        Self::release_bond_liability(env, ch.stake);
    }

    fn release_bond_liability(env: &Env, amount: i128) {
        let held = Self::bonds_held(env) - amount;
        env.storage()
            .instance()
            .set(&DataKey::BondsHeld, &if held > 0 { held } else { 0 });
        Self::bump_instance(env);
    }

    fn bonds_held(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::BondsHeld)
            .unwrap_or(0)
    }

    fn bounty_pool(env: &Env) -> i128 {
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let held = token::Client::new(env, &token_addr).balance(&env.current_contract_address());
        let owed = Self::bonds_held(env);
        if held > owed {
            held - owed
        } else {
            0
        }
    }

    fn record(env: &Env, challenge_id: u64, ch: &Challenge, verdict: Verdict) {
        let mut resolved = ch.clone();
        resolved.verdict = verdict;
        env.storage()
            .persistent()
            .set(&DataKey::Challenge(challenge_id), &resolved);
        Self::bump_instance(env);
        Self::bump(env, &DataKey::Challenge(challenge_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Env,
    };

    /// The freeze is a cross-contract call into the Registry, and
    /// `resolve_by_arbiter` now makes it when it upholds a claim (R3). These
    /// unit tests are not the cross-contract harness, so the Registry is a stub
    /// that accepts `set_claim_freeze` and records nothing. What a freeze
    /// actually does to the collateral is asserted in `integration-tests`.
    #[contract]
    struct RegistryStub;

    #[contractimpl]
    impl RegistryStub {
        pub fn set_claim_freeze(_env: Env, _cert_id: u64, _until: u64) {}
    }

    fn init_client(env: &Env) -> (ChallengeManagerClient<'_>, Address) {
        let registry = env.register(RegistryStub, ());
        let auditor_staking = Address::generate(env);
        let reserve_vault = Address::generate(env);
        let fee_escrow = Address::generate(env);
        let token = Address::generate(env);
        let arbiter = Address::generate(env);
        let treasury = Address::generate(env);

        let contract_id = env.register(ChallengeManager, ());
        let client = ChallengeManagerClient::new(env, &contract_id);
        client.initialize(
            &registry,
            &auditor_staking,
            &reserve_vault,
            &fee_escrow,
            &token,
            &arbiter,
            &treasury,
            &100_0000000i128, // min stake $100
        );
        (client, contract_id)
    }

    #[test]
    #[should_panic(expected = "stake_below_minimum")]
    fn test_challenge_below_min_stake_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = init_client(&env);

        let challenger = Address::generate(&env);
        let victim = Address::generate(&env);
        // $10 < $100 min — panics before any token transfer
        client.challenge(
            &challenger,
            &1u64,
            &ProofType::InsufficientReserve,
            &victim,
            &10_0000000i128,
        );
    }

    /// Seed a pending arbiter-gated claim and its open window directly,
    /// bypassing the token transfer in `challenge()`. `FakeSignature` is the
    /// proof type the arbiter is free to decide either way — a predicate-backed
    /// one may only be confirmed (R4) — and it reads no other contract at
    /// filing.
    fn seed_pending(env: &Env, contract_id: &Address) {
        let challenger = Address::generate(env);
        let victim = Address::generate(env);
        env.as_contract(contract_id, || {
            env.storage().persistent().set(
                &DataKey::Challenge(1u64),
                &Challenge {
                    challenger,
                    cert_id: 1,
                    proof_type: ProofType::FakeSignature,
                    victim,
                    stake: 100_0000000,
                    verdict: Verdict::Pending,
                    filed_at: 0,
                    proven: false,
                    harm: 0,
                    adjudicated: false,
                    arbitrated: false,
                },
            );
            let mut claims = Vec::new(env);
            claims.push_back(1u64);
            env.storage().persistent().set(
                &DataKey::Window(1u64),
                &ClaimWindow {
                    cert_id: 1,
                    opened_at: 0,
                    closes_at: CLAIM_WINDOW_SECONDS,
                    claims,
                },
            );
        });
    }

    /// The arbiter states the quantity, but it must be a quantity: a negative
    /// harm is rejected before the verdict is even loaded.
    #[test]
    #[should_panic(expected = "invalid_harm")]
    fn test_arbiter_negative_harm_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);
        seed_pending(&env, &contract_id);
        client.resolve_by_arbiter(&1u64, &true, &-1i128);
    }

    /// A rejected challenge has no harm to quantify. Stating one anyway is a
    /// contradiction, and fails loudly rather than being silently ignored.
    #[test]
    #[should_panic(expected = "harm_without_verdict")]
    fn test_arbiter_harm_without_a_fraud_verdict_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);
        seed_pending(&env, &contract_id);
        client.resolve_by_arbiter(&1u64, &false, &100_0000000i128);
    }

    /// The arbiter adjudicates one claim; the money still moves at window
    /// close, over the whole window. The recorded verdict and quantity are what
    /// `close_window` later divides by.
    #[test]
    fn test_arbiter_records_verdict_and_quantity_without_settling() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);
        seed_pending(&env, &contract_id);

        client.resolve_by_arbiter(&1u64, &true, &250_0000000i128);

        let ch = client.get_challenge(&1u64);
        assert!(ch.adjudicated);
        assert!(ch.proven);
        assert_eq!(ch.harm, 250_0000000);
        // Nothing settled: the verdict is still Pending until the window closes.
        assert!(ch.verdict == Verdict::Pending);
        assert_eq!(client.window_closes_at(&1u64), CLAIM_WINDOW_SECONDS);
        assert!(!client.is_settled(&1u64));
    }

    /// Adjudication belongs to the window it was filed in. Once the window can
    /// be closed, the arbiter is too late — a settlement that has already been
    /// priced cannot absorb a new number.
    #[test]
    #[should_panic(expected = "claim_window_closed")]
    fn test_arbiter_cannot_adjudicate_after_the_window_closes() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);
        seed_pending(&env, &contract_id);
        env.ledger().set_timestamp(CLAIM_WINDOW_SECONDS);
        client.resolve_by_arbiter(&1u64, &true, &1i128);
    }

    /// A window cannot be closed before its time. Anyone may call it, but not
    /// early — otherwise the aggregation window is whatever the fastest
    /// claimant says it is, which is the race all over again.
    #[test]
    #[should_panic(expected = "claim_window_open")]
    fn test_close_before_the_window_expires_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);
        seed_pending(&env, &contract_id);
        env.ledger().set_timestamp(CLAIM_WINDOW_SECONDS - 1);
        client.close_window(&1u64);
    }

    #[test]
    #[should_panic(expected = "no_open_window")]
    fn test_close_without_a_window_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = init_client(&env);
        client.close_window(&99u64);
    }

    /// The window is 72 hours of ledger time, and it is published so a client
    /// never has to guess.
    #[test]
    fn test_claim_window_is_seventy_two_hours() {
        let env = Env::default();
        let (client, _) = init_client(&env);
        assert_eq!(client.get_claim_window_seconds(), 72 * 60 * 60);
    }
}
