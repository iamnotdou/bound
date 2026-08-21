#![no_std]
//! An executable specification of the PaymentRouter's spend counter.
//!
//! This contract is **never deployed**. It exists so that one question can be
//! settled in code instead of in prose: what does a cumulative spend counter
//! actually prove?
//!
//! The planned `BoundExceeded` proof is `spent > bound`, read from a counter the
//! router increments on every routed payment. That predicate is sound — the
//! arithmetic cannot be forged. The tests below show that it is also cheap to
//! *satisfy honestly* while nobody loses anything, because a counter of gross
//! flow is not a measure of loss.
//!
//! Keep this crate in the workspace. When the real router lands, the same tests
//! should run against it unchanged, and the settlement invariant at the bottom
//! is the one the router must not break.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
pub enum DataKey {
    /// The bound the certificate claims, copied in at `init`.
    Bound,
    /// Cumulative gross value moved. Monotone: nothing decrements it.
    Spent,
    /// Internal balance, exactly as a custodial router would hold it.
    Balance(Address),
    /// Per-recipient tally of value received — the "who was paid" record a
    /// challenge would consult when naming a victim.
    Received(Address),
}

#[contract]
pub struct SpendProbe;

#[contractimpl]
impl SpendProbe {
    pub fn init(env: Env, bound: i128) {
        if env.storage().instance().has(&DataKey::Bound) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::Bound, &bound);
        env.storage().instance().set(&DataKey::Spent, &0i128);
    }

    /// Seed an internal balance. Test-only: the real router mints routed balance
    /// only against underlying USDC it has actually custodied.
    pub fn credit(env: Env, to: Address, amount: i128) {
        let current = Self::balance(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to), &(current + amount));
    }

    /// The router's hot path: move internal balance, record the spend, tally the
    /// recipient. One `require_auth`, no cross-contract calls — the shape the
    /// x402 facilitator rules force.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        if amount <= 0 {
            panic!("invalid_amount");
        }

        let from_balance = Self::balance(env.clone(), from.clone());
        if from_balance < amount {
            panic!("insufficient_balance");
        }
        let to_balance = Self::balance(env.clone(), to.clone());

        env.storage()
            .persistent()
            .set(&DataKey::Balance(from), &(from_balance - amount));
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &(to_balance + amount));

        let spent: i128 = env.storage().instance().get(&DataKey::Spent).unwrap();
        env.storage()
            .instance()
            .set(&DataKey::Spent, &(spent + amount));

        let received = Self::received(env.clone(), to.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Received(to), &(received + amount));
    }

    pub fn balance(env: Env, who: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(who))
            .unwrap_or(0)
    }

    pub fn received(env: Env, who: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Received(who))
            .unwrap_or(0)
    }

    pub fn spent(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Spent).unwrap_or(0)
    }

    pub fn bound(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Bound).unwrap_or(0)
    }

    /// The naive `BoundExceeded` predicate, exactly as the requirement states it.
    pub fn exceeded(env: Env) -> bool {
        Self::spent(env.clone()) > Self::bound(env)
    }
}

#[cfg(test)]
// USDC amounts are written as <dollars>_<7 decimals>, e.g. 100_0000000 is $100.
// Clippy reads that as inconsistent grouping; the grouping is deliberate so the
// dollar figure stays legible.
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    const DOLLAR: i128 = 1_0000000;

    fn setup(env: &Env, bound: i128) -> SpendProbeClient<'_> {
        env.mock_all_auths();
        let id = env.register(SpendProbe, ());
        let client = SpendProbeClient::new(env, &id);
        client.init(&bound);
        client
    }

    /// What a harm-bounded settlement would pay: never more than the harm that
    /// was actually proven, and never more than the collateral behind the
    /// certificate. This is the rule the router's settlement must satisfy.
    fn payable(harm: i128, reserve: i128, allocation: i128) -> i128 {
        let cap = reserve + allocation;
        if harm < cap {
            harm
        } else {
            cap
        }
    }

    /// The $1 shuttle.
    ///
    /// An operator and its agent control two addresses. One dollar of float
    /// moves back and forth between them. Every hop is a real, authorized,
    /// correctly recorded payment, so the counter climbs — and the naive
    /// predicate becomes true with nothing having left the pair.
    #[test]
    fn naive_spend_predicate_is_manufacturable_with_no_net_flow() {
        let env = Env::default();
        let bound = 10_0000000; // $10
        let probe = setup(&env, bound);

        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let outsider = Address::generate(&env);

        // The entire float is one dollar.
        probe.credit(&a, &DOLLAR);

        let mut hops: i128 = 0;
        while !probe.exceeded() {
            if hops % 2 == 0 {
                probe.transfer(&a, &b, &DOLLAR);
            } else {
                probe.transfer(&b, &a, &DOLLAR);
            }
            hops += 1;
        }

        // The proof the challenge would rely on is now true.
        assert!(probe.exceeded());
        assert_eq!(probe.spent(), bound + DOLLAR);

        // It cost bound/unit + 1 payments. At a $1,500 bound that is 1,501
        // transactions — minutes of work and pennies of fees.
        assert_eq!(hops, bound / DOLLAR + 1);

        // And the controlled set is exactly where it started.
        assert_eq!(probe.balance(&a) + probe.balance(&b), DOLLAR);

        // Nobody outside it was touched at all.
        assert_eq!(probe.balance(&outsider), 0);
        assert_eq!(probe.received(&outsider), 0);
    }

    /// Being paid is not being harmed.
    ///
    /// After the shuttle, `b` looks like the most-paid counterparty in the
    /// system — it "received" more than the entire bound. It is holding one
    /// dollar. A victim credential built on the payee tally is self-mintable.
    #[test]
    fn payee_tally_does_not_prove_harm() {
        let env = Env::default();
        let bound = 10_0000000; // $10
        let probe = setup(&env, bound);

        let a = Address::generate(&env);
        let b = Address::generate(&env);

        probe.credit(&a, &DOLLAR);

        // Keep shuttling until b alone has been "paid" more than the entire
        // bound. Each round trip adds one dollar to b's tally, so this costs
        // 2 x bound/unit payments — still trivial.
        let mut hops: i128 = 0;
        while probe.received(&b) <= bound {
            if hops % 2 == 0 {
                probe.transfer(&a, &b, &DOLLAR);
            } else {
                probe.transfer(&b, &a, &DOLLAR);
            }
            hops += 1;
        }

        // b's tally exceeds the whole bound on its own...
        assert!(probe.received(&b) > bound);
        assert_eq!(probe.received(&b), bound + DOLLAR);

        // ...while b is holding one dollar, and its net take is one dollar.
        assert_eq!(probe.balance(&b), DOLLAR);
        assert_eq!(probe.received(&b) - probe.received(&a), DOLLAR);

        // The tallies always sum to the counter, so "who was paid the most" is
        // a ranking the operator writes, not evidence anyone can rely on.
        assert_eq!(probe.received(&a) + probe.received(&b), probe.spent());
    }

    /// The invariant the real settlement must hold: a manufactured breach pays
    /// its manufacturer nothing, because harm — not the counter — sizes the payout.
    #[test]
    fn harm_bounded_settlement_pays_nothing_for_a_manufactured_breach() {
        let env = Env::default();
        let bound = 10_0000000; // $10
        let probe = setup(&env, bound);

        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let victim = Address::generate(&env);

        probe.credit(&a, &DOLLAR);

        let mut hops: i128 = 0;
        while !probe.exceeded() {
            if hops % 2 == 0 {
                probe.transfer(&a, &b, &DOLLAR);
            } else {
                probe.transfer(&b, &a, &DOLLAR);
            }
            hops += 1;
        }

        assert!(probe.exceeded());

        // Value that actually left the controlled set, to the party claiming harm.
        let harm = probe.received(&victim);
        assert_eq!(harm, 0);

        let reserve = 150_0000000; // $150 locked
        let allocation = 100_0000000; // $100 of auditor stake committed
        assert_eq!(payable(harm, reserve, allocation), 0);
    }

    /// The no-fraud complement, so the predicate is not written off as useless:
    /// when the overspend is real, the same counter and the same settlement rule
    /// produce a real payout.
    #[test]
    fn genuine_overspend_to_a_third_party_produces_real_harm() {
        let env = Env::default();
        let bound = 10_0000000; // $10
        let probe = setup(&env, bound);

        let agent = Address::generate(&env);
        let victim = Address::generate(&env);

        probe.credit(&agent, &200_0000000); // $200 of float
        probe.transfer(&agent, &victim, &120_0000000); // $120 out, in one hop

        assert!(probe.exceeded());

        let harm = probe.received(&victim);
        assert_eq!(harm, 120_0000000);
        assert_eq!(probe.balance(&victim), 120_0000000);

        let reserve = 150_0000000;
        let allocation = 100_0000000;
        assert_eq!(payable(harm, reserve, allocation), 120_0000000);
    }

    /// Harm is capped by the collateral actually behind the certificate, so a
    /// large genuine loss cannot reach past the reserve plus the allocation.
    #[test]
    fn payout_is_capped_by_collateral() {
        let reserve = 150_0000000; // $150
        let allocation = 100_0000000; // $100
        let harm = 400_0000000; // $400 genuinely lost

        assert_eq!(payable(harm, reserve, allocation), 250_0000000);
    }
}
