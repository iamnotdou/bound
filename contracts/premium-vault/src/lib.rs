#![no_std]
// USDC amounts are written as <dollars>_<7 decimals>, e.g. 10_0000000 is $10.
// Clippy reads that as inconsistent grouping; the grouping is deliberate so the
// dollar figure stays legible.
#![allow(clippy::inconsistent_digit_grouping)]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

/// The denominator the coverage rate is annualised against.
///
/// A flat 365 days. Not 365.25, not a calendar: the premium is a price, not an
/// interest payment, and a constant a reader can verify by hand is worth more
/// here than four extra hours of accuracy a year.
pub const SECONDS_PER_YEAR: u64 = 31_536_000;

/// Basis-point denominator.
const BPS_DENOM: i128 = 10_000;

/// One certificate's coverage: what the operator paid, what the auditor has
/// earned so far, and what is left to earn.
///
/// Stored under `DataKey::Coverage(cert_id)` — the per-certificate storage style
/// the ReserveVault established. One vault contract serves many certificates and
/// each certificate's money is walled off from every other one: paying for cert
/// A can never fund a claim on cert B, and forfeiting A can never touch B.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Coverage {
    /// The operator who paid. Snapshotted, so a later Registry change cannot
    /// re-point who this coverage belonged to.
    pub payer: Address,
    /// The auditor entitled to the yield, snapshotted at payment time for the
    /// same reason.
    pub auditor: Address,
    /// Total the operator paid, in stroops.
    pub premium: i128,
    /// The protocol's share, already transferred to the treasury at payment
    /// time. Recorded so `premium == protocol_fee + yield_pot` is auditable
    /// from storage alone.
    pub protocol_fee: i128,
    /// `premium - protocol_fee`: the most the auditor can ever receive from
    /// this certificate.
    pub yield_pot: i128,
    /// How much of `yield_pot` the auditor has already withdrawn. Theirs
    /// permanently — see `forfeit`.
    pub claimed: i128,
    /// Coverage start. The certificate's `issued_at`, not the payment time.
    pub start: u64,
    /// Coverage length in seconds: `expires_at - issued_at`.
    pub duration: u64,
    /// Set when the coverage is closed — by a slash (`forfeit`) or by a hygiene
    /// kill (`terminate`). Closing rewrites `yield_pot` to the exact amount
    /// frozen in place and stops accrual dead: from then on `accrued` returns
    /// `yield_pot` outright and the clock is irrelevant.
    pub closed: bool,
    /// The ledger instant the coverage was closed at. A record, not an input to
    /// the accrual arithmetic — see `closed`.
    pub closed_at: u64,
}

#[contracttype]
pub enum DataKey {
    Registry,
    ChallengeManager,
    Token,
    /// Where the protocol fee share and every forfeited stroop go. Named once
    /// at `initialize`, with no admin and no setter — the same rule the
    /// ChallengeManager's treasury follows, and for the same reason: a mutable
    /// destination for forfeited money is a prize somebody can aim.
    Treasury,
    /// The annualised coverage rate, in basis points of the bound.
    RateBps,
    /// The protocol's share of each premium, in basis points of the premium.
    FeeBps,
    Coverage(u64),
}

#[contract]
pub struct PremiumVault;

#[contractimpl]
impl PremiumVault {
    /// `rate_bps` and `fee_bps` are **simple, transparent, configurable
    /// parameters**, deliberately.
    ///
    /// There is no actuarial model here, no risk tiering and no external
    /// underwriter. The premium is `bound × rate × duration`, annualised, and
    /// nothing else. Risk-based pricing would need a loss history this protocol
    /// does not have, and inventing one in code would be a lie dressed as a
    /// model. When there is real loss data, `rate_bps` is the single number that
    /// changes — at a fresh deployment, since there is no admin.
    pub fn initialize(
        env: Env,
        registry: Address,
        challenge_manager: Address,
        token: Address,
        treasury: Address,
        rate_bps: i128,
        fee_bps: i128,
    ) {
        if env.storage().instance().has(&DataKey::Registry) {
            panic!("already_initialized");
        }
        if rate_bps < 0 {
            panic!("invalid_rate");
        }
        // A fee share above 100% would mean the auditor's pot is negative.
        if !(0..=BPS_DENOM).contains(&fee_bps) {
            panic!("invalid_fee");
        }
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage()
            .instance()
            .set(&DataKey::ChallengeManager, &challenge_manager);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::RateBps, &rate_bps);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
    }

    // ----- pricing -------------------------------------------------------

    /// The price of covering `bound` for `duration_seconds`.
    ///
    /// ```text
    /// premium = bound * rate_bps * duration_seconds / (10_000 * SECONDS_PER_YEAR)
    /// ```
    ///
    /// **Truncation.** Integer division truncates toward zero, and the remainder
    /// is deliberately dropped rather than rounded. That direction is chosen,
    /// not accidental: the operator is charged **no more** than the exact price,
    /// which keeps the quote a hard ceiling a counterparty can verify by hand.
    /// The same truncation is applied to the fee share and to accrual, where it
    /// runs the other way — the auditor accrues **no more** than the exact
    /// figure — so the vault can never owe out more than it holds. Every one of
    /// these errors is at most one stroop (10⁻⁷ USDC).
    ///
    /// Concretely, a $1,500 bound at 200 bps for 90 days is
    /// `15_000_000_000 * 200 * 7_776_000 / (10_000 * 31_536_000)`
    /// `= 73_972_602` stroops (exactly $7.3972602 before truncation of the
    /// trailing `.7397…`).
    ///
    /// **Overflow.** `overflow-checks = true` is on for release, but this uses
    /// explicit `checked_mul` so a hostile `bound` or `duration` produces a
    /// named contract error rather than relying on a profile setting that a
    /// future `Cargo.toml` edit could switch off.
    pub fn quote(env: Env, bound: i128, duration_seconds: u64) -> i128 {
        let rate_bps: i128 = env.storage().instance().get(&DataKey::RateBps).unwrap();
        Self::price(bound, rate_bps, duration_seconds)
    }

    /// The price this certificate would pay, read from its own `bound` and its
    /// own coverage window. Callable before payment, so an operator can see the
    /// number before committing to it.
    pub fn quote_cert(env: Env, cert_id: u64) -> i128 {
        let bound = Self::cert_bound(&env, cert_id);
        let duration = Self::cert_duration(&env, cert_id);
        let rate_bps: i128 = env.storage().instance().get(&DataKey::RateBps).unwrap();
        Self::price(bound, rate_bps, duration)
    }

    // ----- lifecycle -----------------------------------------------------

    /// The operator pays this certificate's coverage premium. Once, ever.
    ///
    /// **Which duration.** `expires_at - issued_at`, both immutable fields of
    /// the certificate — *not* `expires_at - now`. Two reasons, and the second
    /// is the load-bearing one:
    ///
    /// 1. Coverage runs from the moment the certificate exists, so that is the
    ///    period being priced.
    /// 2. Pricing from `now` would make the premium a function of **when the
    ///    operator chooses to call this**. An operator would simply wait, and a
    ///    year's coverage would be bought for a day's price the instant before
    ///    expiry. Anchoring to `issued_at` removes the choice entirely: the
    ///    price is fixed at publish and no transaction timing can move it.
    ///
    /// The certificate must be attested — the premium is yield on **staked**
    /// capital, so there has to be an auditor with capital allocated to it
    /// before there is anything to accrue to.
    ///
    /// A zero-duration or zero-bound certificate prices at zero and is recorded
    /// as paid without moving a stroop. It does not panic: the Registry already
    /// rejects both at `publish`, and a vault that panicked on an impossible
    /// input would be one more way for a caller to get an unexplained failure.
    pub fn pay_premium(env: Env, cert_id: u64) {
        if env.storage().persistent().has(&DataKey::Coverage(cert_id)) {
            panic!("premium_already_paid");
        }

        // Authenticate against *this certificate's* operator, read live from the
        // Registry. Any operator can cover their own certificate; nobody can pay
        // for — or bind coverage to — somebody else's.
        let operator = Self::cert_operator(&env, cert_id);
        operator.require_auth();

        if !Self::cert_is_verified(&env, cert_id) {
            panic!("cert_not_verified");
        }
        let auditor = Self::cert_auditor(&env, cert_id);

        let bound = Self::cert_bound(&env, cert_id);
        let duration = Self::cert_duration(&env, cert_id);
        let start = Self::cert_issued_at(&env, cert_id);
        let rate_bps: i128 = env.storage().instance().get(&DataKey::RateBps).unwrap();
        let premium = Self::price(bound, rate_bps, duration);

        let fee_bps: i128 = env.storage().instance().get(&DataKey::FeeBps).unwrap();
        // Truncates down, so the dust lands in the auditor's pot rather than the
        // treasury's. At most one stroop, and it favours the party that carries
        // the risk.
        let protocol_fee = premium.checked_mul(fee_bps).expect("premium_overflow") / BPS_DENOM;
        let yield_pot = premium - protocol_fee;

        if premium > 0 {
            let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            let client = token::Client::new(&env, &token_addr);
            client.transfer(&operator, &env.current_contract_address(), &premium);
            // The protocol's share leaves immediately rather than being held and
            // released later. Holding it would create a second pot with its own
            // release rules, its own bug surface, and — as `fee-escrow` proved —
            // its own way to get stuck. There is nothing to decide later: the
            // fee is not contingent on anything.
            if protocol_fee > 0 {
                let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
                client.transfer(&env.current_contract_address(), &treasury, &protocol_fee);
            }
        }

        env.storage().persistent().set(
            &DataKey::Coverage(cert_id),
            &Coverage {
                payer: operator,
                auditor,
                premium,
                protocol_fee,
                yield_pot,
                claimed: 0,
                start,
                duration,
                closed: false,
                closed_at: 0,
            },
        );
    }

    /// Total yield accrued to this certificate's auditor so far, capped at
    /// `yield_pot` and never falling.
    ///
    /// Straight-line: `yield_pot * elapsed / duration`. Before `start` it is
    /// zero; at or past `start + duration` it is the whole pot; after the
    /// coverage is closed it is frozen at the closing instant. There is no path
    /// by which this exceeds `yield_pot`, including arbitrarily far past expiry.
    pub fn accrued(env: Env, cert_id: u64) -> i128 {
        match Self::coverage_of(&env, cert_id) {
            None => 0,
            Some(c) => Self::accrued_at(&env, &c),
        }
    }

    /// What the auditor could withdraw right now: accrued minus already claimed.
    pub fn claimable(env: Env, cert_id: u64) -> i128 {
        match Self::coverage_of(&env, cert_id) {
            None => 0,
            Some(c) => {
                let a = Self::accrued_at(&env, &c);
                if a > c.claimed {
                    a - c.claimed
                } else {
                    0
                }
            }
        }
    }

    /// The auditor withdraws accrued-and-unclaimed yield.
    ///
    /// **Claiming is allowed at any time, including mid-coverage.** Straight-line
    /// accrual makes that the natural reading — at every instant the accrued
    /// figure is precisely payment for coverage already delivered, and making the
    /// auditor wait until expiry would be an interest-free loan from the auditor
    /// to the protocol for no security gain.
    ///
    /// It is safe *because* of the forfeiture rule, not in spite of it. Forfeiture
    /// only ever takes **unclaimed** yield, so an auditor who claims continuously
    /// is converting forfeitable yield into settled income as fast as they earn
    /// it. That is deliberate and it is honestly priced: the auditor's skin in
    /// the game is their **allocation**, which stays fully slashable however fast
    /// they claim. The premium is yield on that capital, not a second bond, and
    /// pretending an unclaimed premium is collateral would over-state the
    /// protocol's teeth.
    ///
    /// The cost, stated plainly: a diligent auditor who claims every block
    /// forfeits almost nothing on a slash. The forfeiture is a real but modest
    /// consequence, and the allocation is what actually punishes a bad
    /// attestation.
    pub fn claim(env: Env, cert_id: u64) -> i128 {
        let mut c = Self::coverage_of(&env, cert_id).expect("no_coverage");
        c.auditor.require_auth();

        let accrued = Self::accrued_at(&env, &c);
        let amount = accrued - c.claimed;
        if amount <= 0 {
            return 0;
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &c.auditor,
            &amount,
        );

        c.claimed = accrued;
        env.storage()
            .persistent()
            .set(&DataKey::Coverage(cert_id), &c);
        amount
    }

    // ----- settlement, called only by the ChallengeManager ----------------

    /// **Step 4 of the settlement waterfall.** The auditor was slashed for this
    /// certificate, so they forfeit **unclaimed** yield on it.
    ///
    /// The split, and the reason for each half:
    ///
    /// - **Accrued-but-unclaimed → the victim**, capped by `victim_cap` (the
    ///   harm the operator's own reserve did not already cover). This is
    ///   consistent with the waterfall's first invariant — *victim compensation
    ///   comes only from the operator's own money* — because **the premium is
    ///   the operator's money**. Every stroop in this pot was paid in by the
    ///   operator of this certificate and has not yet been handed to anybody
    ///   else. Compensating a victim from it is the same left-pocket-to-right
    ///   move that makes self-dealing a wash on the reserve.
    /// - **Everything above the cap, and the entire unaccrued remainder → the
    ///   treasury.** The cap is what preserves the second invariant, *everything
    ///   stays capped by proven harm*: the victim can never receive more than
    ///   the harm that was proven, no matter how large the premium is. The
    ///   unaccrued remainder is nobody's — it is payment for coverage that will
    ///   now never be delivered — and the treasury is the protocol's existing
    ///   neutral sink. It is deliberately **not** refunded to the operator: this
    ///   path only runs when the certificate has been proven harmful, and
    ///   refunding the operator would pay them back for the coverage they broke.
    ///
    /// **Nothing here can pay the auditor's stake to anybody.** This function
    /// cannot reach `AuditorStaking` at all; it moves only tokens this contract
    /// already holds, which arrived from the operator.
    ///
    /// Already-claimed yield is untouched and untouchable. There is no clawback
    /// and no attempt at one — the auditor has been paid and the money is gone
    /// from this contract. Writing a clawback that cannot work would be a lie in
    /// the code.
    ///
    /// A certificate that never paid a premium settles as a silent no-op, so the
    /// waterfall does not care whether coverage was bought.
    pub fn forfeit(env: Env, cert_id: u64, victim: Address, victim_cap: i128) -> i128 {
        Self::require_challenge_manager(&env);
        if victim_cap < 0 {
            panic!("invalid_cap");
        }

        let mut c = match Self::coverage_of(&env, cert_id) {
            None => return 0,
            Some(c) => c,
        };
        if c.closed {
            return 0;
        }

        let now = env.ledger().timestamp();
        let accrued = Self::accrued_at(&env, &c);
        let accrued_unclaimed = accrued - c.claimed;
        let unaccrued = c.yield_pot - accrued;

        let to_victim = if accrued_unclaimed < victim_cap {
            accrued_unclaimed
        } else {
            victim_cap
        };
        let to_treasury = accrued_unclaimed - to_victim + unaccrued;

        // The auditor forfeits by having claimed everything they will ever
        // claim: `claimed` is left where it is and the pot is zeroed, so
        // `claimable` is 0 from here forward whatever the clock does.
        c.yield_pot = c.claimed;
        c.closed = true;
        c.closed_at = now;
        env.storage()
            .persistent()
            .set(&DataKey::Coverage(cert_id), &c);

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let client = token::Client::new(&env, &token_addr);
        if to_victim > 0 {
            client.transfer(&env.current_contract_address(), &victim, &to_victim);
        }
        if to_treasury > 0 {
            let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
            client.transfer(&env.current_contract_address(), &treasury, &to_treasury);
        }

        to_victim
    }

    /// **Hygiene mode.** The certificate was killed on a true proof that
    /// evidences no harm, so nobody was hurt and the auditor was not slashed.
    ///
    /// The premium is handled the only way consistent with that: accrual stops
    /// at the kill, the auditor **keeps** what accrued up to it and can still
    /// claim it, and the unaccrued remainder goes to the treasury.
    ///
    /// - The auditor keeps the accrued share because hygiene mode explicitly
    ///   does not blame them — they covered the period they covered.
    /// - The unaccrued remainder is not refunded to the operator, for the same
    ///   reason DESIGN-V2 §10 already gives: a dead certificate is supposed to
    ///   cost the operator their certificate, their reserve lockup **and their
    ///   premium**. Refunding it would make killing your own certificate free,
    ///   which is exactly the manufacturable breach hygiene mode exists to price.
    pub fn terminate(env: Env, cert_id: u64) -> i128 {
        Self::require_challenge_manager(&env);

        let mut c = match Self::coverage_of(&env, cert_id) {
            None => return 0,
            Some(c) => c,
        };
        if c.closed {
            return 0;
        }

        let now = env.ledger().timestamp();
        let accrued = Self::accrued_at(&env, &c);
        let unaccrued = c.yield_pot - accrued;

        c.yield_pot = accrued;
        c.closed = true;
        c.closed_at = now;
        env.storage()
            .persistent()
            .set(&DataKey::Coverage(cert_id), &c);

        if unaccrued > 0 {
            let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();
            token::Client::new(&env, &token_addr).transfer(
                &env.current_contract_address(),
                &treasury,
                &unaccrued,
            );
        }

        unaccrued
    }

    // ----- views ---------------------------------------------------------

    pub fn get_coverage(env: Env, cert_id: u64) -> Coverage {
        Self::coverage_of(&env, cert_id).expect("no_coverage")
    }

    pub fn is_paid(env: Env, cert_id: u64) -> bool {
        env.storage().persistent().has(&DataKey::Coverage(cert_id))
    }

    pub fn get_premium(env: Env, cert_id: u64) -> i128 {
        Self::coverage_of(&env, cert_id)
            .map(|c| c.premium)
            .unwrap_or(0)
    }

    pub fn get_claimed(env: Env, cert_id: u64) -> i128 {
        Self::coverage_of(&env, cert_id)
            .map(|c| c.claimed)
            .unwrap_or(0)
    }

    pub fn get_rate_bps(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::RateBps).unwrap()
    }

    pub fn get_fee_bps(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::FeeBps).unwrap()
    }

    pub fn get_treasury(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Treasury).unwrap()
    }

    // ----- internals -----------------------------------------------------

    fn price(bound: i128, rate_bps: i128, duration_seconds: u64) -> i128 {
        if bound <= 0 || rate_bps <= 0 || duration_seconds == 0 {
            return 0;
        }
        bound
            .checked_mul(rate_bps)
            .expect("premium_overflow")
            .checked_mul(duration_seconds as i128)
            .expect("premium_overflow")
            / (BPS_DENOM * SECONDS_PER_YEAR as i128)
    }

    /// Total accrued to the auditor, capped at `yield_pot` and monotone.
    ///
    /// **Closing freezes it.** `forfeit` and `terminate` both rewrite
    /// `yield_pot` to the exact figure that is frozen in place — the claimed
    /// total for a forfeiture, the accrued total for a hygiene termination — and
    /// set `closed`. From then on this returns that number outright rather than
    /// re-deriving it from the clock. Re-deriving would be wrong twice over: it
    /// would apply the elapsed fraction a second time to an already-reduced pot,
    /// and it would keep moving as the ledger advanced past a coverage that has
    /// ended.
    fn accrued_at(env: &Env, c: &Coverage) -> i128 {
        if c.yield_pot <= 0 {
            return 0;
        }
        if c.closed {
            return c.yield_pot;
        }
        let now = env.ledger().timestamp();
        // A zero-length coverage period is fully accrued the moment it exists.
        // Unreachable through the Registry, which rejects a non-future expiry,
        // but a division guard is cheaper than trusting that forever.
        if c.duration == 0 {
            return c.yield_pot;
        }
        if now <= c.start {
            return 0;
        }
        let elapsed = now - c.start;
        if elapsed >= c.duration {
            return c.yield_pot;
        }
        // Truncates down: the auditor accrues no more than the exact figure, so
        // the pot can never be over-drawn by rounding.
        c.yield_pot
            .checked_mul(elapsed as i128)
            .expect("accrual_overflow")
            / c.duration as i128
    }

    fn coverage_of(env: &Env, cert_id: u64) -> Option<Coverage> {
        env.storage().persistent().get(&DataKey::Coverage(cert_id))
    }

    fn require_challenge_manager(env: &Env) {
        let cm: Address = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeManager)
            .unwrap();
        cm.require_auth();
    }

    fn registry(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::Registry).unwrap()
    }

    fn cert_operator(env: &Env, cert_id: u64) -> Address {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_operator"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn cert_auditor(env: &Env, cert_id: u64) -> Address {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_auditor"),
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

    fn cert_is_verified(env: &Env, cert_id: u64) -> bool {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "is_cert_verified"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn cert_issued_at(env: &Env, cert_id: u64) -> u64 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_issued_at"),
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

    /// The priced coverage window: `expires_at - issued_at`, saturating at zero
    /// rather than panicking on a certificate whose expiry is not after its
    /// issue.
    fn cert_duration(env: &Env, cert_id: u64) -> u64 {
        Self::cert_expires_at(env, cert_id).saturating_sub(Self::cert_issued_at(env, cert_id))
    }
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
