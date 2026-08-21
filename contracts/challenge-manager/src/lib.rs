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
    ChallengeWins,
    ChallengeFails,
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
}

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
    }

    pub fn get_premium_vault(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::PremiumVault)
            .expect("premium_vault_not_set")
    }

    /// Whether step 4 is live. False means a deployment that never called
    /// `set_premium_vault`, in which case settlement silently skips the premium
    /// step — see the comment on step 4 in `settle_fraud`.
    pub fn has_premium_vault(env: Env) -> bool {
        env.storage().instance().has(&DataKey::PremiumVault)
    }

    // Anyone can submit a challenge — must post a bond to prevent spam.
    // `victim` is the counterparty to be compensated if fraud is proven.
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

        // Bond moves into the ChallengeManager and stays at risk until resolution
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

        env.storage().persistent().set(
            &DataKey::Challenge(challenge_id),
            &Challenge {
                challenger,
                cert_id,
                proof_type,
                victim,
                stake,
                verdict: Verdict::Pending,
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::ChallengeCount, &challenge_id);
        env.storage()
            .instance()
            .set(&DataKey::BondsHeld, &(Self::bonds_held(&env) + stake));

        challenge_id
    }

    // Trustless resolution. The contract proves the fraud itself by reading
    // on-chain state — no oracle, no human.
    //
    // ---------------------------------------------------------------------
    // WHY `BoundExceeded` AND `ExpiredCertificate` SETTLE IN HYGIENE MODE
    //
    // Read this before "fixing" either of them to slash the auditor. The
    // hygiene settlement is the security property, not an unfinished job.
    //
    // Both predicates are TRUE, and both are MANUFACTURABLE AT WILL BY THE
    // OPERATOR. `contracts/spend-probe` and the router's own shuttle test prove
    // it for `BoundExceeded`: a $1 shuttle between two addresses the operator
    // controls drives `spent` past any bound for the price of gas, with a net
    // flow out of the operator's control of exactly zero. Every hop is a real,
    // authorized, correctly recorded payment — there is nothing to detect and
    // nothing to forge. `ExpiredCertificate` is the same shape: the operator
    // controls whether their own agent keeps paying after expiry.
    //
    // So if either slashed the auditor on the counter alone, ANY OPERATOR COULD
    // DESTROY THEIR AUDITOR'S ALLOCATION FOR THE COST OF GAS. The slash goes to
    // the treasury, so the operator gains nothing by it — but the auditor loses
    // everything, at will, with no defence available to them. An auditing
    // business cannot exist under that rule, and the protocol is worthless
    // without auditors.
    //
    // What these proofs establish is that THE COVENANT WAS BROKEN, not that
    // ANYONE WAS HARMED. The certificate dying is the correct and sufficient
    // automatic consequence: it costs the operator their own certificate, their
    // reserve lockup and (later) their premium, and it warns every counterparty
    // reading `verify`. Compensation and slashing require ASSESSED HARM, which
    // is exactly what `resolve_by_arbiter(challenge_id, fraud_proven, harm)` is
    // for — a human states the number, and the same waterfall spends it.
    //
    // `InsufficientReserve` is different and keeps the full waterfall: its harm
    // is a shortfall in capital the operator promised and did not commit. That
    // is an amount, not a counter, and it cannot be manufactured without the
    // operator actually losing the reserve they withheld.
    // ---------------------------------------------------------------------
    pub fn resolve(env: Env, challenge_id: u64) {
        let ch = Self::load_pending(&env, challenge_id);

        let fraud = match ch.proof_type {
            ProofType::InsufficientReserve => Self::verify_insufficient_reserve(&env, ch.cert_id),
            ProofType::BoundExceeded => Self::verify_bound_exceeded(&env, ch.cert_id),
            ProofType::ExpiredCertificate => Self::verify_expired_certificate(&env, ch.cert_id),
            // A forged auditor signature leaves no on-chain trace to read.
            ProofType::FakeSignature => panic!("needs_arbiter"),
        };

        if fraud {
            // Harm is arithmetic here: the contract computes it from state.
            let harm = Self::raw_harm(&env, &ch);
            Self::settle_fraud(&env, challenge_id, &ch, harm);
        } else {
            Self::settle_no_fraud(&env, challenge_id);
        }
    }

    /// Arbiter-gated resolution for subjective proof types (BoundExceeded,
    /// FakeSignature) that no contract can verify on-chain. This is an explicit
    /// trust assumption: the arbiter is named at initialize().
    ///
    /// The arbiter states the **quantity** as well as the verdict, and that
    /// `harm` feeds the same waterfall every other proof type uses.
    ///
    /// WHY THAT IS SAFE. The arbiter is already a fully trusted party for these
    /// proof types — they are the ones deciding whether fraud occurred at all,
    /// so letting them also state the amount grants no new trust. And their
    /// number is bounded by exactly the same rails as an arithmetic one:
    /// `payable = min(harm, reserve + allocation)` caps the total outflow,
    /// victim compensation still comes only from the operator's own reserve for
    /// this certificate, and the slash still goes only to the treasury. An
    /// arbiter who overstates harm therefore cannot direct money to anyone who
    /// could have bribed them — the self-dealing property is preserved by the
    /// waterfall, not by the predicate.
    ///
    /// The alternative — a verdict with no quantity — is worse, and was the
    /// behaviour this replaced: every arbiter proof settled in hygiene mode, so
    /// an auditor who genuinely vouched for a certificate whose agent blew
    /// through its bound walked away whole purely because the proof happened to
    /// be arbiter-gated rather than arithmetic.
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

        let ch = Self::load_pending(&env, challenge_id);

        if fraud_proven {
            // harm == 0 is legitimate and lands in hygiene mode: a real breach
            // that nobody can evidence a loss for.
            Self::settle_fraud(&env, challenge_id, &ch, harm);
        } else {
            Self::settle_no_fraud(&env, challenge_id);
        }
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

    fn load_pending(env: &Env, challenge_id: u64) -> Challenge {
        let ch: Challenge = env
            .storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found");
        if ch.verdict != Verdict::Pending {
            panic!("already_resolved");
        }
        ch
    }

    // On-chain proof: the live reserve balance is below what the certificate claims.
    fn verify_insufficient_reserve(env: &Env, cert_id: u64) -> bool {
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

        actual < claimed
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
    /// evaluate-at-filing machinery does not exist yet. It errs toward not
    /// killing a certificate, and it is recorded as an open gap in
    /// `docs/DESIGN-V2.md` §10 rather than silently accepted.
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

    /// Raw harm an **on-chain** predicate proves, in stroops. Zero means "the
    /// covenant was broken but no harm is evidenced on-chain" — see
    /// `settle_hygiene`. Arbiter-gated proof types never reach this function:
    /// their harm is stated by the arbiter and passed straight to
    /// `settle_fraud`.
    ///
    /// `InsufficientReserve`: the shortfall, claimed reserve minus actual. The
    /// named victim is deliberately NOT part of this number. A named victim is a
    /// filter, not a proof: receiving a payment is evidence of being paid, not
    /// of being harmed. The waterfall neutralises a self-named victim rather
    /// than the predicate trying to authenticate one.
    fn raw_harm(env: &Env, ch: &Challenge) -> i128 {
        match ch.proof_type {
            ProofType::InsufficientReserve => {
                let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
                let vault: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ReserveVault)
                    .unwrap();
                let claimed: i128 = env.invoke_contract(
                    &registry,
                    &Symbol::new(env, "get_cert_reserve"),
                    Vec::from_array(env, [ch.cert_id.into_val(env)]),
                );
                let actual: i128 = env.invoke_contract(
                    &vault,
                    &Symbol::new(env, "get_balance"),
                    Vec::from_array(env, [ch.cert_id.into_val(env)]),
                );
                if claimed > actual {
                    claimed - actual
                } else {
                    0
                }
            }
            // `BoundExceeded` and `ExpiredCertificate` reach this arm, and the
            // zero is the whole point: both are provable from router state and
            // both are manufacturable by the operator, so neither may size a
            // payout. Zero routes them to `settle_hygiene` — certificate dead,
            // flat bounty, reserve untouched, auditor not slashed. Read the
            // comment on `resolve` before changing this to anything else.
            //
            // `FakeSignature` never arrives here: `resolve` refuses it and the
            // arbiter path supplies its own quantity.
            _ => 0,
        }
    }

    /// `harm` is supplied by the caller: computed from chain state by
    /// `resolve`, stated by the trusted arbiter in `resolve_by_arbiter`. From
    /// here down the two are indistinguishable and travel identical rails.
    fn settle_fraud(env: &Env, challenge_id: u64, ch: &Challenge, harm: i128) {
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
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );
        let allocation: i128 = env.invoke_contract(
            &staking,
            &Symbol::new(env, "get_allocation"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        // Nothing provable to pay for: kill the certificate, pay the flat
        // bounty, leave the money where it is.
        if harm <= 0 {
            Self::settle_hygiene(env, challenge_id, ch, &registry, &staking);
            return;
        }

        // The ceiling on everything below. Total outflow can never exceed the
        // harm actually proven, nor the collateral this certificate carries.
        let payable = if harm < reserve + allocation {
            harm
        } else {
            reserve + allocation
        };

        // 1. Victim compensation — from the OPERATOR'S OWN RESERVE for THIS
        //    certificate only.
        //
        //    ATTACK CLOSED: self-dealing. A colluding operator that names an
        //    address it controls as "victim" is moving money from its left
        //    pocket to its right. Manufacturing a proof against yourself
        //    extracts nothing, so the permissiveness of victim naming stops
        //    mattering.
        let victim_amount = if payable < reserve { payable } else { reserve };
        if victim_amount > 0 {
            Self::pay_from_reserve(env, &vault, ch.cert_id, &ch.victim, victim_amount);
        }
        let reserve_left = reserve - victim_amount;

        // 2. Challenger fee — a percentage of PROVEN HARM, out of the same
        //    reserve, never out of the stake.
        //
        //    ATTACK CLOSED: bounty-hunting auditors. v1 paid 20% of the auditor's
        //    live stake, so the most profitable target was whoever had posted the
        //    most collateral, regardless of how small the fraud was.
        let mut fee = harm * CHALLENGER_FEE_BPS / 10_000;
        if fee > reserve_left {
            fee = reserve_left;
        }
        if fee > 0 {
            Self::pay_from_reserve(env, &vault, ch.cert_id, &ch.challenger, fee);
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
        //    capped by that certificate's allocation.
        let residual = payable - victim_amount;
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
                        ch.cert_id.into_val(env),
                        treasury.into_val(env),
                        slash.into_val(env),
                    ],
                ),
            );
        }

        // 4. Forfeited premium. The auditor vouched for a certificate that has
        //    now been proven harmful, so they forfeit the yield they have not
        //    already withdrawn. `PremiumVault::forfeit` splits it: accrued-but-
        //    unclaimed to the victim, capped below; everything else — the excess
        //    over the cap plus the entire unaccrued remainder — to the treasury.
        //
        //    CONSISTENT WITH RULE 1, and this is the whole reason it is allowed
        //    to reach the victim at all: **the premium is the operator's own
        //    money.** Every stroop in that pot was paid in by this certificate's
        //    operator and has not yet been handed to anyone. Paying a victim out
        //    of it is the same left-pocket-to-right move that makes a
        //    self-dealing operator's "compensation" a wash.
        //
        //    CONSISTENT WITH RULE 3: nothing here can move the auditor's stake.
        //    The vault has no reference to AuditorStaking and moves only tokens
        //    it already holds.
        //
        //    THE CAP is `harm - victim_amount`: the harm the operator's own
        //    reserve did not already cover. Without it, a large premium could
        //    pay a victim more than the harm proven against the certificate,
        //    breaking the waterfall's "capped by proven harm" invariant.
        //
        //    WHY `payable` IS NOT RAISED BY THE FORFEITED PREMIUM. It was
        //    considered and rejected, because it is arithmetically a no-op and a
        //    readability loss. `payable` only ever binds through two mins:
        //    `victim_amount = min(payable, reserve)` and
        //    `slash = min(payable - victim_amount, allocation)`. Adding a
        //    premium `P` gives `payable' = min(harm, reserve + allocation + P)`.
        //    If `harm < reserve + allocation` nothing changes at all. Otherwise
        //    `victim_amount` is `reserve` either way, and `slash` is capped at
        //    `allocation` either way. So folding `P` into `payable` changes
        //    neither line — while making steps 1 and 3 read as though the
        //    premium could enlarge them. Keeping `payable` as the cap on
        //    *collateral* (reserve + allocation), and giving the premium its own
        //    explicit cap here, is the same amount of money with none of the
        //    ambiguity. It also honours what the original gap note promised:
        //    step 4 arrives without disturbing the amounts above it.
        if let Some(pv) = Self::premium_vault(env) {
            let victim_cap = harm - victim_amount;
            env.invoke_contract::<i128>(
                &pv,
                &Symbol::new(env, "forfeit"),
                Vec::from_array(
                    env,
                    [
                        ch.cert_id.into_val(env),
                        ch.victim.clone().into_val(env),
                        victim_cap.into_val(env),
                    ],
                ),
            );
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
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        // 6. Kill the certificate and return the challenger's bond.
        Self::invalidate(env, &registry, ch.cert_id);
        Self::return_bond(env, ch);

        Self::record(env, challenge_id, ch, Verdict::ChallengeWins);
    }

    /// Hygiene mode: the proof is real, but nobody can evidence harm.
    ///
    /// The certificate is invalidated, the allocation retires in full, the
    /// challenger is paid a FIXED bounty out of forfeited bonds, and the
    /// operator's reserve is not touched at all. A dead certificate dies without
    /// a payout being invented for it.
    fn settle_hygiene(
        env: &Env,
        challenge_id: u64,
        ch: &Challenge,
        registry: &Address,
        staking: &Address,
    ) {
        env.invoke_contract::<()>(
            staking,
            &Symbol::new(env, "retire_allocation"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
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
                Vec::from_array(env, [ch.cert_id.into_val(env)]),
            );
        }

        Self::invalidate(env, registry, ch.cert_id);
        Self::return_bond(env, ch);

        // Paid from forfeited bonds only — failed challengers fund successful
        // hygiene challenges. There is no other pot it could come from: the
        // reserve is off limits by definition here, and paying it out of the
        // stake would hand a challenger a slice of the auditor's money, which is
        // the thing rule 3 exists to forbid. If the pool is short, the bounty is
        // whatever the pool holds, including nothing.
        let pool = Self::bounty_pool(env);
        let bounty = if HYGIENE_BOUNTY < pool {
            HYGIENE_BOUNTY
        } else {
            pool
        };
        if bounty > 0 {
            let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            token::Client::new(env, &token_addr).transfer(
                &env.current_contract_address(),
                &ch.challenger,
                &bounty,
            );
        }

        Self::record(env, challenge_id, ch, Verdict::ChallengeWins);
    }

    // Challenge failed: the challenger forfeits their bond. It stays in the
    // contract and stops being owed back, which is what makes it available as
    // hygiene bounty for a future, genuine challenge.
    fn settle_no_fraud(env: &Env, challenge_id: u64) {
        let mut ch: Challenge = env
            .storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found");
        ch.verdict = Verdict::ChallengeFails;
        env.storage()
            .persistent()
            .set(&DataKey::Challenge(challenge_id), &ch);
        Self::release_bond_liability(env, ch.stake);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn init_client(env: &Env) -> (ChallengeManagerClient<'_>, Address) {
        let registry = Address::generate(env);
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

    /// Seed a pending challenge directly, bypassing the token transfer in
    /// `challenge()`. `BoundExceeded` keeps `settle_no_fraud` from touching any
    /// other contract.
    fn seed_pending(env: &Env, contract_id: &Address) {
        let challenger = Address::generate(env);
        let victim = Address::generate(env);
        env.as_contract(contract_id, || {
            env.storage().persistent().set(
                &DataKey::Challenge(1u64),
                &Challenge {
                    challenger,
                    cert_id: 1,
                    proof_type: ProofType::BoundExceeded,
                    victim,
                    stake: 100_0000000,
                    verdict: Verdict::Pending,
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

    #[test]
    #[should_panic(expected = "already_resolved")]
    fn test_double_resolve_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);

        seed_pending(&env, &contract_id);

        // Arbiter rules "no fraud" → resolved. Second call must panic.
        client.resolve_by_arbiter(&1u64, &false, &0i128);
        client.resolve_by_arbiter(&1u64, &false, &0i128);
    }
}
