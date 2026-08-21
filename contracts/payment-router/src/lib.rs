#![no_std]
//! # PaymentRouter — a SEP-41 token that is also the protocol's spend meter.
//!
//! ## Read this before you use `spent()` for anything
//!
//! The spend counter is **evidence that the covenant was broken**. It is NOT
//! proof that anyone was harmed, and it must never be a payout trigger on its
//! own.
//!
//! `contracts/spend-probe` is the executable proof of that claim, and
//! `payment_router_reproduces_the_dollar_shuttle` below reproduces it against
//! this contract. Cumulative routed spend measures **gross flow**, not loss. A
//! single dollar shuttled between two addresses one operator controls drives
//! `spent` past any bound for the price of gas: every hop is a real, authorized,
//! correctly recorded payment, and nothing leaves the operator's control.
//!
//! So `spent(cert_id) > bound` is sound as a *predicate* — the arithmetic cannot
//! be forged — and worthless as a *settlement rule*. Anything that pays out must
//! size the payout by harm proven to a party outside the operator's control, and
//! cap it by the collateral actually behind the certificate. Wire this counter
//! straight to a payout and you have built a faucet, not an insurance policy.
//!
//! ## Why this contract holds custody instead of wrapping the SAC
//!
//! x402 facilitator settlement requires that paying for a resource is exactly
//! one `transfer(from, to, amount)` call on the asset contract, emitting exactly
//! one transfer event, with no sub-invocations. A router that called through to
//! the underlying USDC SAC on `transfer` would emit two transfer events and a
//! nested invocation, and settlement would break. So the router custodies the
//! underlying USDC (via `deposit`/`withdraw`, which MAY call the SAC) and moves
//! purely **internal** balances on `transfer`. `transfer_meter_makes_no_sub_
//! invocation` proves the hot path stays flat.
//!
//! Because `transfer` may not call out, everything the hot path needs — the
//! agent's certificate, its `expires_at`, its halt flag — is copied into router
//! storage at `enroll` time and read locally afterwards.

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, IntoVal, String,
    Symbol, Vec,
};

/// Matches the Stellar USDC asset contract, which this token wraps 1:1.
const DECIMALS: u32 = 7;

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct AllowanceKey {
    pub from: Address,
    pub spender: Address,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AllowanceValue {
    pub amount: i128,
    pub expiration_ledger: u32,
}

/// The hot-path copy of an agent's certificate.
///
/// `expires_at` is snapshotted at `enroll` so that `transfer` can decide whether
/// a payment is post-expiry without invoking the Registry. Certificate expiry is
/// immutable in the Registry once published, so the snapshot cannot go stale.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct Enrollment {
    pub cert_id: u64,
    pub expires_at: u64,
}

/// Everything a `spent > bound`-style predicate needs in order to apply §7's
/// grace window and de-minimis floor **later**. This contract records; it does
/// not judge. Applying the window or the floor here would bake two contested
/// parameters into a redeploy-only surface.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct PostExpiry {
    /// Cumulative value routed strictly after `expires_at`.
    pub total: i128,
    /// Number of post-expiry payments.
    pub count: u32,
    /// The largest single post-expiry payment, and when it settled — the pair a
    /// de-minimis floor is applied to.
    pub max_payment: i128,
    pub max_payment_at: u64,
    /// When the first post-expiry payment settled — the timestamp a grace window
    /// is measured against.
    pub first_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct CertConfig {
    /// The operator named on the certificate, snapshotted at first enrollment.
    /// Only this address may halt, resume, or change the cap.
    pub operator: Address,
    /// §6 float cap: the most underlying USDC the router will accept into this
    /// certificate. This is the number that bounds what a stolen agent key can
    /// reach — a thief cannot spend float the operator never deposited.
    pub float_cap: i128,
    /// §6 kill switch. Operator-only, both directions.
    pub halted: bool,
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
    /// The underlying USDC SAC held in custody.
    Token,
    /// Total internal supply. Invariant: equals the router's USDC balance.
    Supply,
    Balance(Address),
    Allowance(AllowanceKey),
    /// agent -> Enrollment. Presence is what "tracked" means (§8).
    Enrolled(Address),
    Cert(u64),
    /// Underlying USDC currently held on behalf of this certificate.
    Float(u64),
    /// Cumulative gross flow routed by this certificate's agents. Monotone.
    Spent(u64),
    PostExpiry(u64),
}

#[contract]
pub struct PaymentRouter;

#[contractimpl]
impl PaymentRouter {
    // -----------------------------------------------------------------------
    // Setup
    // -----------------------------------------------------------------------

    pub fn initialize(env: Env, registry: Address, token: Address) {
        if env.storage().instance().has(&DataKey::Registry) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Supply, &0i128);
        Self::bump_instance(&env);
    }

    // -----------------------------------------------------------------------
    // Enrollment — what makes an agent tracked (§8)
    // -----------------------------------------------------------------------

    /// Bind `agent` to `cert_id` and set that certificate's float cap.
    ///
    /// **Both the agent and the operator must authorize, and neither alone is
    /// enough.**
    ///
    /// The operator, because enrollment attaches spend to *their* certificate:
    /// the counter it feeds is the evidence a challenge will read, and the float
    /// cap it sets is a claim about their own collateral. If the agent could
    /// enroll alone, anyone could bind an address they control to a stranger's
    /// certificate and manufacture a `spent > bound` record against them.
    ///
    /// The agent, because enrollment is not free to the agent either: it
    /// subjects every one of that address's transfers to metering and puts the
    /// address under the operator's kill switch. You may not conscript an
    /// address you do not control. If the operator could enroll alone, they
    /// could freeze an unrelated party's balance by halting.
    ///
    /// The operator address is read live from the Registry, so authority follows
    /// the certificate rather than a local admin field.
    pub fn enroll(env: Env, agent: Address, cert_id: u64, float_cap: i128) {
        agent.require_auth();

        let operator = Self::cert_operator(&env, cert_id);
        operator.require_auth();

        if float_cap <= 0 {
            panic!("invalid_float_cap");
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Enrolled(agent.clone()))
        {
            // An agent's binding is permanent, whether the second enrollment
            // names the same certificate or a different one. Re-binding would
            // let an operator walk an agent off a certificate whose counter is
            // climbing and onto a fresh one, which would make the counter
            // worthless as evidence. A new agent address is the supported path.
            panic!("already_enrolled");
        }

        let expires_at = Self::cert_expires_at(&env, cert_id);
        env.storage().persistent().set(
            &DataKey::Enrolled(agent.clone()),
            &Enrollment {
                cert_id,
                expires_at,
            },
        );

        // The first enrollment fixes the certificate's config; later agents on
        // the same certificate inherit it rather than resetting the cap.
        if !env.storage().persistent().has(&DataKey::Cert(cert_id)) {
            env.storage().persistent().set(
                &DataKey::Cert(cert_id),
                &CertConfig {
                    operator,
                    float_cap,
                    halted: false,
                },
            );
        }

        // The enrollment and the certificate config are what make an agent
        // tracked. If either archived, `is_tracked` would abort rather than
        // answer, and with it the meter, the kill switch and the float cap.
        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Enrolled(agent.clone()));
        Self::bump(&env, &DataKey::Cert(cert_id));

        env.events()
            .publish((symbol_short!("enroll"), agent), (cert_id, float_cap));
    }

    pub fn is_tracked(env: Env, agent: Address) -> bool {
        env.storage().persistent().has(&DataKey::Enrolled(agent))
    }

    /// The certificate an agent is bound to, or `None` if untracked.
    pub fn cert_of(env: Env, agent: Address) -> Option<u64> {
        Self::enrollment(&env, &agent).map(|e| e.cert_id)
    }

    // -----------------------------------------------------------------------
    // Metering views
    // -----------------------------------------------------------------------

    /// Cumulative **gross flow** routed by this certificate. See the header:
    /// this is not a measure of loss and must never size a payout on its own.
    pub fn spent(env: Env, cert_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Spent(cert_id))
            .unwrap_or(0)
    }

    /// Gross flow routed strictly after the certificate's `expires_at`, plus the
    /// records §7's grace window and de-minimis floor need. Neither is applied
    /// here.
    pub fn post_expiry_spent(env: Env, cert_id: u64) -> PostExpiry {
        Self::post_expiry_of(&env, cert_id)
    }

    // -----------------------------------------------------------------------
    // Float cap (§6)
    // -----------------------------------------------------------------------

    pub fn float(env: Env, cert_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Float(cert_id))
            .unwrap_or(0)
    }

    pub fn float_cap(env: Env, cert_id: u64) -> i128 {
        Self::cert_config(&env, cert_id).float_cap
    }

    /// Operator-only. Lowering the cap does not claw back float already held; it
    /// only refuses further deposits.
    pub fn set_float_cap(env: Env, cert_id: u64, float_cap: i128) {
        let mut cfg = Self::cert_config(&env, cert_id);
        cfg.operator.require_auth();
        if float_cap <= 0 {
            panic!("invalid_float_cap");
        }
        let old = cfg.float_cap;
        cfg.float_cap = float_cap;
        env.storage()
            .persistent()
            .set(&DataKey::Cert(cert_id), &cfg);
        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Cert(cert_id));
        env.events()
            .publish((symbol_short!("cap_set"), cert_id), (old, float_cap));
    }

    // -----------------------------------------------------------------------
    // Kill switch (§6)
    // -----------------------------------------------------------------------

    /// Halt routing for a certificate. Operator-only, and deliberately
    /// independent of the challenge system: compromise response must not wait on
    /// a challenge, and halting neither invalidates the certificate nor exposes
    /// the auditor to a slash.
    ///
    /// While halted, no enrolled agent of this certificate can move value —
    /// `transfer`, `transfer_from`, `burn`, `burn_from` and `withdraw` are all
    /// refused. `transfer_from` is included because otherwise an allowance the
    /// thief granted themselves *before* the halt would survive it and keep
    /// draining the float, which would defeat the point of halting first.
    /// Allowances are left recorded, not deleted, so a halt/resume cycle does
    /// not silently destroy legitimate standing approvals.
    ///
    /// The agent key cannot clear this: `resume` authenticates against the
    /// certificate's operator, which is exactly the address a thief holding the
    /// agent key does not have.
    ///
    /// Halting freezes the float rather than rescuing it — `withdraw` is gated
    /// too, so the honest operator cannot get their own money out either. See
    /// `clawback`, which is the recovery door that does not re-arm the thief.
    pub fn halt(env: Env, cert_id: u64) {
        Self::set_halted(&env, cert_id, true);
        env.events().publish((symbol_short!("halt"), cert_id), ());
    }

    pub fn resume(env: Env, cert_id: u64) {
        Self::set_halted(&env, cert_id, false);
        env.events().publish((symbol_short!("resume"), cert_id), ());
    }

    pub fn is_halted(env: Env, cert_id: u64) -> bool {
        Self::cert_config(&env, cert_id).halted
    }

    /// Sweep a halted certificate's agent balance back to the operator (§6).
    ///
    /// ## The problem this exists to solve
    ///
    /// `halt` gates `transfer`, `transfer_from`, `burn`, `burn_from` **and**
    /// `withdraw`. `withdraw` has to be gated, or a thief holding the agent key
    /// simply withdraws the float and the kill switch is decorative. But that
    /// gate is symmetric: it also stops the *honest operator* from recovering
    /// their own float, and the only way out was `resume` — which re-arms the
    /// thief. The operator was made to choose between losing the money and
    /// handing it back to the attacker. `clawback` is the third door.
    ///
    /// ## Why this is safe
    ///
    /// Two independent reasons, and both matter:
    ///
    /// 1. **The thief cannot call it.** Authority is the certificate's
    ///    **operator**, read live from the Registry at enrollment and stored in
    ///    `CertConfig` — exactly the address a thief holding the *agent* key
    ///    does not have. It is the same authority that gates `halt`, `resume`
    ///    and `set_float_cap`.
    /// 2. **The money cannot reach a stranger.** The destination is not a
    ///    parameter. It is the operator, and nothing else. And the float is the
    ///    operator's own committed capital in the first place — this returns
    ///    their money to them, it does not reach anyone else's.
    ///
    /// ## Two decisions, stated rather than buried
    ///
    /// **No timelock, deliberately.** A timelock on clawback would be a delay
    /// handed to the thief: the whole point of the halt/clawback pair is that
    /// compromise response is faster than the attacker. A 24-hour clawback
    /// timelock gives an attacker who has *already* got the agent key 24 hours
    /// to find any path out of the router that the halt did not close. And a
    /// timelock buys nothing here that is worth that: the standard reason to
    /// timelock a privileged sweep is to give *depositors* time to exit ahead
    /// of an abusive admin, and there are no third-party depositors on this
    /// balance — see the next paragraph.
    ///
    /// **An operator can sweep their agent's float at will, halt or no halt —
    /// and that is acceptable.** Say it plainly: outside a compromise, nothing
    /// stops an operator halting their own certificate and clawing the float
    /// back at any moment. The agent's routed balance is money the operator
    /// deposited under a cap the operator set, and the operator could already
    /// reach all of it by halting and then `withdraw`ing after a resume. So
    /// clawback creates **no new authority over anyone else's money** — it only
    /// removes the requirement to re-arm the thief on the way. What it does
    /// change is the *cost of the sweep* to the operator: it is now a one-call,
    /// no-window operation. An agent is not a custodian and must never be
    /// treated as one. Anyone extending this router so that a **third party**
    /// can hold a balance under an enrolled agent's certificate must revisit
    /// this function first, because that person's money would be inside the
    /// sweep. Recorded in `docs/THREAT-MODEL.md` as residue, not as a fix.
    ///
    /// Returns the amount swept. Sweeping an already-empty balance is a no-op
    /// that returns `0` rather than panicking: clawback is compromise-response
    /// plumbing, and a retry during an incident must not fail loudly for having
    /// nothing left to do.
    pub fn clawback(env: Env, cert_id: u64, agent: Address) -> i128 {
        let cfg = Self::cert_config(&env, cert_id);
        cfg.operator.require_auth();

        // Only while halted. Clawback is not a routine withdrawal path — the
        // halt is the operator's own on-chain statement that this certificate
        // is in incident response, and it is what makes the sweep legible to
        // anybody reading the ledger afterwards.
        if !cfg.halted {
            panic!("not_halted");
        }

        // The agent must belong to *this* certificate. `cert_id` is not
        // redundant with the enrollment: passing both and checking them against
        // each other means a mistyped agent address on a certificate the caller
        // does not operate fails the operator check instead of sweeping a
        // stranger.
        //
        // The agent is a parameter because a certificate may have several
        // enrolled agents (`enroll` lets later agents inherit the config) and
        // the router keeps no certificate -> agents index. Sweeping one named
        // agent per call is the honest shape; a compromise of two keys is two
        // calls.
        match Self::enrollment(&env, &agent) {
            Some(e) if e.cert_id == cert_id => {}
            _ => panic!("agent_not_enrolled"),
        }

        let amount = Self::balance(env.clone(), agent.clone());
        if amount == 0 {
            return 0;
        }

        // A pure internal balance move: no USDC leaves custody and `Supply` is
        // untouched, so `total_supply() == usdc(router)` still holds. The
        // operator can `withdraw` from their own balance afterwards on their
        // own authority — that path is not halted, because the operator is not
        // an enrolled agent of the certificate.
        Self::debit(&env, &agent, amount);
        Self::credit(&env, &cfg.operator, amount);

        // The float moves with the balance, using exactly the same rule the hot
        // path uses: it leaves the agent's certificate, and lands on the
        // operator's certificate only if the operator is themselves an enrolled
        // agent somewhere (they normally are not).
        Self::reduce_float(&env, cert_id, amount);
        if let Some(e) = Self::enrollment(&env, &cfg.operator) {
            Self::raise_float(&env, e.cert_id, amount);
        }

        // **The meter is deliberately not touched.** `spent` is evidence that
        // the covenant was broken, and a recovery is not a payment. Metering a
        // clawback would let an operator inflate their own `spent` counter —
        // and, worse, would let a recovery look like the very breach a
        // `BoundExceeded` challenge reads.
        //
        // Routing also stays halted. Clawback neither performs nor implies a
        // resume, so the thief is never re-enabled; recovering the money and
        // re-opening the certificate stay two separate, deliberate acts.

        // Both events on purpose. The `clawback` event is the one an operator
        // audits; the SEP-41 `transfer` event is what keeps any indexer
        // tracking router balances from desynchronising. Emitting `transfer`
        // here does not touch the x402 constraint, which is about what the
        // `transfer` *function* may do on the hot path.
        env.events().publish(
            (
                symbol_short!("transfer"),
                agent.clone(),
                cfg.operator.clone(),
            ),
            amount,
        );
        env.events()
            .publish((symbol_short!("clawback"), cert_id, agent), amount);

        amount
    }

    // -----------------------------------------------------------------------
    // Wrapping the underlying USDC
    // -----------------------------------------------------------------------

    /// Move underlying USDC into custody and credit an equal internal balance.
    ///
    /// This one MAY call the SAC — only `transfer` is constrained by x402.
    pub fn deposit(env: Env, from: Address, amount: i128) {
        from.require_auth();
        Self::check_amount(amount);

        // §6: a deposit that would push the certificate's float above its cap is
        // refused. This is the number that bounds a stolen agent key's reach.
        if let Some(e) = Self::enrollment(&env, &from) {
            let cfg = Self::cert_config(&env, e.cert_id);
            let next = Self::float(env.clone(), e.cert_id)
                .checked_add(amount)
                .expect("float_overflow");
            if next > cfg.float_cap {
                panic!("float_cap_exceeded");
            }
            Self::set_float(&env, e.cert_id, next);
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &from,
            &env.current_contract_address(),
            &amount,
        );

        Self::credit(&env, &from, amount);
        Self::set_supply(
            &env,
            Self::supply_of(&env)
                .checked_add(amount)
                .expect("supply_overflow"),
        );

        env.events()
            .publish((symbol_short!("deposit"), from), amount);
    }

    /// Burn internal balance and release the underlying USDC back to its holder.
    pub fn withdraw(env: Env, to: Address, amount: i128) {
        to.require_auth();
        Self::check_amount(amount);
        Self::reject_if_halted(&env, &to);

        Self::debit(&env, &to, amount);
        Self::set_supply(&env, Self::supply_of(&env) - amount);
        if let Some(e) = Self::enrollment(&env, &to) {
            Self::reduce_float(&env, e.cert_id, amount);
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );

        env.events()
            .publish((symbol_short!("withdraw"), to), amount);
    }

    // -----------------------------------------------------------------------
    // SEP-41
    // -----------------------------------------------------------------------

    /// The x402 hot path.
    ///
    /// One `require_auth`, an internal balance move, the meter, and exactly one
    /// `transfer` event. No cross-contract call happens here and none may ever
    /// be added — see the header.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        Self::do_transfer(&env, &from, &to, amount);
    }

    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        Self::spend_allowance(&env, &from, &spender, amount);
        Self::do_transfer(&env, &from, &to, amount);
    }

    pub fn approve(
        env: Env,
        from: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        from.require_auth();
        if amount < 0 {
            panic!("invalid_amount");
        }
        if amount > 0 && expiration_ledger < env.ledger().sequence() {
            panic!("expiration_in_past");
        }

        // Deliberately NOT TTL-extended (defect L2). Allowances live in
        // temporary storage and SEP-41 gives them their own expiry: the caller
        // names `expiration_ledger` and `allowance_value` refuses anything past
        // it. Extending the entry's lifetime beyond the expiry the approver
        // chose would be the contract overriding them; letting it archive early
        // fails closed, to an allowance of zero.
        env.storage().temporary().set(
            &DataKey::Allowance(AllowanceKey {
                from: from.clone(),
                spender: spender.clone(),
            }),
            &AllowanceValue {
                amount,
                expiration_ledger,
            },
        );
        Self::bump_instance(&env);

        env.events().publish(
            (symbol_short!("approve"), from, spender),
            (amount, expiration_ledger),
        );
    }

    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        Self::allowance_value(&env, &from, &spender).amount
    }

    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(id))
            .unwrap_or(0)
    }

    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        Self::do_burn(&env, &from, amount);
    }

    pub fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        Self::spend_allowance(&env, &from, &spender, amount);
        Self::do_burn(&env, &from, amount);
    }

    pub fn decimals(_env: Env) -> u32 {
        DECIMALS
    }

    pub fn name(env: Env) -> String {
        String::from_str(&env, "Bound Routed USDC")
    }

    pub fn symbol(env: Env) -> String {
        String::from_str(&env, "bUSDC")
    }

    /// Total internal supply. Invariant, asserted in the tests: this equals the
    /// router's balance of the underlying USDC. Custody is never fractional.
    pub fn total_supply(env: Env) -> i128 {
        Self::supply_of(&env)
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    fn do_transfer(env: &Env, from: &Address, to: &Address, amount: i128) {
        Self::check_amount(amount);
        Self::reject_if_halted(env, from);

        Self::debit(env, from, amount);
        Self::credit(env, to, amount);

        // The meter. Only a tracked payer moves the counter: an untracked
        // address is not under any certificate's covenant, so its flow is not
        // that certificate's evidence.
        if let Some(e) = Self::enrollment(env, from) {
            Self::meter(env, &e, to, amount);
            Self::reduce_float(env, e.cert_id, amount);
        }
        if let Some(e) = Self::enrollment(env, to) {
            // Value arriving at a tracked agent is float the router now holds
            // for that certificate. It is counted, but not cap-checked: refusing
            // an inbound payment would make an honest agent unable to be paid,
            // and inbound value is not something a stolen key can conjure. The
            // cap constrains what the operator commits, which is the exposure
            // §6 is about.
            Self::raise_float(env, e.cert_id, amount);
        }

        // Exactly one transfer event, in the standard SEP-41 shape.
        env.events().publish(
            (symbol_short!("transfer"), from.clone(), to.clone()),
            amount,
        );
    }

    fn meter(env: &Env, e: &Enrollment, to: &Address, amount: i128) {
        let spent = Self::spent(env.clone(), e.cert_id)
            .checked_add(amount)
            .expect("spent_overflow");
        env.storage()
            .persistent()
            .set(&DataKey::Spent(e.cert_id), &spent);
        // The spend counter is the evidence `BoundExceeded` and
        // `ExpiredCertificate` are proven from. It has to stay readable past
        // the certificate's own expiry, which is exactly when those proofs get
        // filed.
        Self::bump(env, &DataKey::Spent(e.cert_id));

        let now = env.ledger().timestamp();
        if now > e.expires_at {
            let mut pe = Self::post_expiry_of(env, e.cert_id);
            pe.total = pe.total.checked_add(amount).expect("spent_overflow");
            pe.count = pe.count.checked_add(1).expect("spent_overflow");
            if pe.count == 1 {
                pe.first_at = now;
            }
            if amount > pe.max_payment {
                pe.max_payment = amount;
                pe.max_payment_at = now;
            }
            env.storage()
                .persistent()
                .set(&DataKey::PostExpiry(e.cert_id), &pe);
            Self::bump(env, &DataKey::PostExpiry(e.cert_id));
        }

        env.events().publish(
            (symbol_short!("spend"), e.cert_id),
            (to.clone(), amount, spent),
        );
    }

    fn do_burn(env: &Env, from: &Address, amount: i128) {
        Self::check_amount(amount);
        Self::reject_if_halted(env, from);
        Self::debit(env, from, amount);
        Self::set_supply(env, Self::supply_of(env) - amount);
        if let Some(e) = Self::enrollment(env, from) {
            Self::reduce_float(env, e.cert_id, amount);
        }
        env.events()
            .publish((symbol_short!("burn"), from.clone()), amount);
    }

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

    fn check_amount(amount: i128) {
        if amount <= 0 {
            panic!("invalid_amount");
        }
    }

    fn debit(env: &Env, who: &Address, amount: i128) {
        let balance = Self::balance(env.clone(), who.clone());
        if balance < amount {
            panic!("insufficient_balance");
        }
        env.storage()
            .persistent()
            .set(&DataKey::Balance(who.clone()), &(balance - amount));
        Self::bump_instance(env);
        Self::bump(env, &DataKey::Balance(who.clone()));
    }

    fn credit(env: &Env, who: &Address, amount: i128) {
        let balance = Self::balance(env.clone(), who.clone())
            .checked_add(amount)
            .expect("balance_overflow");
        env.storage()
            .persistent()
            .set(&DataKey::Balance(who.clone()), &balance);
        Self::bump_instance(env);
        Self::bump(env, &DataKey::Balance(who.clone()));
    }

    fn allowance_value(env: &Env, from: &Address, spender: &Address) -> AllowanceValue {
        let key = DataKey::Allowance(AllowanceKey {
            from: from.clone(),
            spender: spender.clone(),
        });
        match env.storage().temporary().get::<_, AllowanceValue>(&key) {
            Some(v) if v.expiration_ledger >= env.ledger().sequence() => v,
            _ => AllowanceValue {
                amount: 0,
                expiration_ledger: 0,
            },
        }
    }

    fn spend_allowance(env: &Env, from: &Address, spender: &Address, amount: i128) {
        Self::check_amount(amount);
        let current = Self::allowance_value(env, from, spender);
        if current.amount < amount {
            panic!("insufficient_allowance");
        }
        env.storage().temporary().set(
            &DataKey::Allowance(AllowanceKey {
                from: from.clone(),
                spender: spender.clone(),
            }),
            &AllowanceValue {
                amount: current.amount - amount,
                expiration_ledger: current.expiration_ledger,
            },
        );
    }

    fn enrollment(env: &Env, agent: &Address) -> Option<Enrollment> {
        env.storage()
            .persistent()
            .get(&DataKey::Enrolled(agent.clone()))
    }

    fn cert_config(env: &Env, cert_id: u64) -> CertConfig {
        env.storage()
            .persistent()
            .get(&DataKey::Cert(cert_id))
            .expect("cert_not_enrolled")
    }

    fn set_halted(env: &Env, cert_id: u64, halted: bool) {
        let mut cfg = Self::cert_config(env, cert_id);
        cfg.operator.require_auth();
        cfg.halted = halted;
        env.storage()
            .persistent()
            .set(&DataKey::Cert(cert_id), &cfg);
        Self::bump_instance(env);
        Self::bump(env, &DataKey::Cert(cert_id));
    }

    fn reject_if_halted(env: &Env, from: &Address) {
        if let Some(e) = Self::enrollment(env, from) {
            if Self::cert_config(env, e.cert_id).halted {
                panic!("cert_halted");
            }
        }
    }

    fn post_expiry_of(env: &Env, cert_id: u64) -> PostExpiry {
        env.storage()
            .persistent()
            .get(&DataKey::PostExpiry(cert_id))
            .unwrap_or(PostExpiry {
                total: 0,
                count: 0,
                max_payment: 0,
                max_payment_at: 0,
                first_at: 0,
            })
    }

    fn set_float(env: &Env, cert_id: u64, amount: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::Float(cert_id), &amount);
        Self::bump_instance(env);
        Self::bump(env, &DataKey::Float(cert_id));
    }

    fn raise_float(env: &Env, cert_id: u64, amount: i128) {
        let next = Self::float(env.clone(), cert_id)
            .checked_add(amount)
            .expect("float_overflow");
        Self::set_float(env, cert_id, next);
    }

    /// Float never goes negative: the balance check in `debit` already bounds
    /// the outflow, and saturating here keeps a rounding surprise from bricking
    /// the hot path.
    fn reduce_float(env: &Env, cert_id: u64, amount: i128) {
        let next = Self::float(env.clone(), cert_id)
            .saturating_sub(amount)
            .max(0);
        Self::set_float(env, cert_id, next);
    }

    fn supply_of(env: &Env) -> i128 {
        env.storage().instance().get(&DataKey::Supply).unwrap_or(0)
    }

    fn set_supply(env: &Env, amount: i128) {
        env.storage().instance().set(&DataKey::Supply, &amount);
        Self::bump_instance(env);
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

    fn cert_expires_at(env: &Env, cert_id: u64) -> u64 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_expires_at"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }
}

#[cfg(test)]
mod test;
