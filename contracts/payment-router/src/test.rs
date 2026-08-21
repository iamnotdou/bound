//! USDC amounts are written as `<dollars>_<7 decimals>`, e.g. `100_0000000` is
//! $100. Clippy reads that as inconsistent grouping; the grouping is deliberate
//! so the dollar figure stays legible.
#![allow(clippy::inconsistent_digit_grouping)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token, IntoVal, Symbol, TryFromVal, Val,
};

const DOLLAR: i128 = 1_0000000;
const CERT: u64 = 1;
const EXPIRES_AT: u64 = 10_000;
const CAP: i128 = 100_0000000; // $100

/// A stand-in for the Registry exposing only the two views the router reads.
#[contract]
pub struct MockRegistry;

#[contracttype]
pub enum MockKey {
    Operator(u64),
    ExpiresAt(u64),
}

#[contractimpl]
impl MockRegistry {
    pub fn set_cert(env: Env, cert_id: u64, operator: Address, expires_at: u64) {
        env.storage()
            .persistent()
            .set(&MockKey::Operator(cert_id), &operator);
        env.storage()
            .persistent()
            .set(&MockKey::ExpiresAt(cert_id), &expires_at);
    }

    pub fn get_cert_operator(env: Env, cert_id: u64) -> Address {
        env.storage()
            .persistent()
            .get(&MockKey::Operator(cert_id))
            .expect("certificate_not_found")
    }

    pub fn get_cert_expires_at(env: Env, cert_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&MockKey::ExpiresAt(cert_id))
            .expect("certificate_not_found")
    }
}

struct World<'a> {
    env: Env,
    router: PaymentRouterClient<'a>,
    router_id: Address,
    usdc: Address,
    operator: Address,
    agent: Address,
}

impl World<'_> {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000);

        let usdc_admin = Address::generate(&env);
        let usdc = env
            .register_stellar_asset_contract_v2(usdc_admin.clone())
            .address();

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);

        let registry = env.register(MockRegistry, ());
        MockRegistryClient::new(&env, &registry).set_cert(&CERT, &operator, &EXPIRES_AT);

        let router_id = env.register(PaymentRouter, ());
        let router = PaymentRouterClient::new(&env, &router_id);
        router.initialize(&registry, &usdc);

        let w = World {
            env,
            router,
            router_id,
            usdc,
            operator,
            agent,
        };
        w.mint(&w.agent, 10_000_0000000);
        w
    }

    fn mint(&self, to: &Address, amount: i128) {
        token::StellarAssetClient::new(&self.env, &self.usdc).mint(to, &amount);
    }

    fn usdc_balance(&self, who: &Address) -> i128 {
        token::Client::new(&self.env, &self.usdc).balance(who)
    }

    fn addr(&self) -> Address {
        Address::generate(&self.env)
    }

    /// Enrolled agent, funded with `amount` of routed balance.
    fn enrolled(&self, amount: i128) {
        self.router.enroll(&self.agent, &CERT, &CAP);
        self.router.deposit(&self.agent, &amount);
    }
}

/// The router's custody must never be fractional.
fn assert_fully_backed(w: &World) {
    assert_eq!(w.router.total_supply(), w.usdc_balance(&w.router_id));
}

// ---------------------------------------------------------------------------
// SEP-41 conformance
// ---------------------------------------------------------------------------

#[test]
fn metadata_matches_the_underlying_usdc() {
    let w = World::new();
    assert_eq!(w.router.decimals(), 7);
    assert_eq!(
        w.router.name(),
        String::from_str(&w.env, "Bound Routed USDC")
    );
    assert_eq!(w.router.symbol(), String::from_str(&w.env, "bUSDC"));
}

#[test]
fn deposit_and_withdraw_wrap_and_unwrap_one_to_one() {
    let w = World::new();
    let before = w.usdc_balance(&w.agent);

    w.router.deposit(&w.agent, &50_0000000);
    assert_eq!(w.router.balance(&w.agent), 50_0000000);
    assert_eq!(w.usdc_balance(&w.agent), before - 50_0000000);
    assert_eq!(w.usdc_balance(&w.router_id), 50_0000000);
    assert_fully_backed(&w);

    w.router.withdraw(&w.agent, &20_0000000);
    assert_eq!(w.router.balance(&w.agent), 30_0000000);
    assert_eq!(w.usdc_balance(&w.agent), before - 30_0000000);
    assert_fully_backed(&w);
}

#[test]
fn transfer_moves_internal_balances() {
    let w = World::new();
    let payee = w.addr();
    w.router.deposit(&w.agent, &10_0000000);

    w.router.transfer(&w.agent, &payee, &4_0000000);
    assert_eq!(w.router.balance(&w.agent), 6_0000000);
    assert_eq!(w.router.balance(&payee), 4_0000000);
    // Custody is untouched: no underlying USDC moved.
    assert_eq!(w.usdc_balance(&w.router_id), 10_0000000);
    assert_fully_backed(&w);
}

#[test]
fn transfer_beyond_balance_fails_and_moves_nothing() {
    let w = World::new();
    let payee = w.addr();
    w.router.deposit(&w.agent, &DOLLAR);

    assert!(w
        .router
        .try_transfer(&w.agent, &payee, &(2 * DOLLAR))
        .is_err());
    assert_eq!(w.router.balance(&w.agent), DOLLAR);
    assert_eq!(w.router.balance(&payee), 0);
}

#[test]
fn approve_allowance_and_transfer_from() {
    let w = World::new();
    let spender = w.addr();
    let payee = w.addr();
    w.router.deposit(&w.agent, &10_0000000);

    assert_eq!(w.router.allowance(&w.agent, &spender), 0);
    w.router.approve(&w.agent, &spender, &6_0000000, &1_000u32);
    assert_eq!(w.router.allowance(&w.agent, &spender), 6_0000000);

    w.router
        .transfer_from(&spender, &w.agent, &payee, &4_0000000);
    assert_eq!(w.router.allowance(&w.agent, &spender), 2_0000000);
    assert_eq!(w.router.balance(&w.agent), 6_0000000);
    assert_eq!(w.router.balance(&payee), 4_0000000);

    // Beyond the remaining allowance is refused.
    assert!(w
        .router
        .try_transfer_from(&spender, &w.agent, &payee, &3_0000000)
        .is_err());
    assert_eq!(w.router.balance(&payee), 4_0000000);
}

#[test]
fn expired_allowance_is_not_spendable() {
    let w = World::new();
    let spender = w.addr();
    let payee = w.addr();
    w.router.deposit(&w.agent, &10_0000000);
    w.router.approve(&w.agent, &spender, &6_0000000, &5u32);

    w.env.ledger().set_sequence_number(6);
    assert_eq!(w.router.allowance(&w.agent, &spender), 0);
    assert!(w
        .router
        .try_transfer_from(&spender, &w.agent, &payee, &DOLLAR)
        .is_err());
}

#[test]
fn burn_and_burn_from_reduce_balance_and_supply() {
    let w = World::new();
    let spender = w.addr();
    w.router.deposit(&w.agent, &10_0000000);

    w.router.burn(&w.agent, &3_0000000);
    assert_eq!(w.router.balance(&w.agent), 7_0000000);
    assert_eq!(w.router.total_supply(), 7_0000000);

    w.router.approve(&w.agent, &spender, &5_0000000, &1_000u32);
    w.router.burn_from(&spender, &w.agent, &2_0000000);
    assert_eq!(w.router.balance(&w.agent), 5_0000000);
    assert_eq!(w.router.total_supply(), 5_0000000);
    assert_eq!(w.router.allowance(&w.agent, &spender), 3_0000000);

    // Burning leaves the underlying USDC stranded in custody, which is why the
    // supply invariant is a `<=` after a burn rather than an equality.
    assert!(w.router.total_supply() <= w.usdc_balance(&w.router_id));
}

// ---------------------------------------------------------------------------
// The x402 constraint
// ---------------------------------------------------------------------------

/// The property x402 facilitator settlement depends on: paying for a resource is
/// exactly one `transfer` call, emitting exactly one transfer event, with no
/// sub-invocation.
///
/// Proved three ways, because any one of them alone is circumstantial:
///   1. exactly one event in the standard `("transfer", from, to) -> amount`
///      shape is emitted by the router and nothing else;
///   2. the authorized invocation tree for the call has no sub-invocations;
///   3. the underlying USDC ledger does not move at all, so no SAC call happened.
#[test]
fn transfer_emits_exactly_one_event_and_makes_no_sub_invocation() {
    let w = World::new();
    let payee = w.addr();
    w.router.deposit(&w.agent, &10_0000000);

    let usdc_before = w.usdc_balance(&w.router_id);

    w.router.transfer(&w.agent, &payee, &4_0000000);

    // (1) One event for the whole invocation, and it is the transfer event.
    let events = w.env.events().all();
    assert_eq!(events.len(), 1, "untracked transfer emits one event");
    let (contract, topics, data) = events.last().unwrap();
    assert_eq!(contract, w.router_id);
    let expected: Vec<Val> =
        (symbol_short!("transfer"), w.agent.clone(), payee.clone()).into_val(&w.env);
    assert_eq!(topics, expected);
    assert_eq!(i128::try_from_val(&w.env, &data).unwrap(), 4_0000000);

    // (2) A flat authorization tree — nothing was invoked underneath.
    let auths = w.env.auths();
    assert_eq!(auths.len(), 1);
    assert_eq!(auths[0].0, w.agent);
    assert!(
        auths[0].1.sub_invocations.is_empty(),
        "transfer must not invoke another contract"
    );

    // (3) Custody untouched: the SAC was never called.
    assert_eq!(w.usdc_balance(&w.router_id), usdc_before);
}

/// The same property for a *tracked* payer, where the meter also runs. Metering
/// adds a `spend` event for indexers, but it must not add a second `transfer`
/// event and must not add a sub-invocation — those are the two things x402
/// actually constrains.
#[test]
fn metered_transfer_still_emits_exactly_one_transfer_event() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(10_0000000);

    w.router.transfer(&w.agent, &payee, &4_0000000);
    let events = w.env.events().all();
    assert_eq!(events.len(), 2, "one transfer event plus one spend event");

    let transfers = events
        .iter()
        .filter(|(_, topics, _)| {
            Symbol::try_from_val(&w.env, &topics.get(0).unwrap()).unwrap()
                == symbol_short!("transfer")
        })
        .count();
    assert_eq!(transfers, 1, "exactly one transfer event");

    assert!(w.env.auths()[0].1.sub_invocations.is_empty());
}

// ---------------------------------------------------------------------------
// Metering
// ---------------------------------------------------------------------------

#[test]
fn unenrolled_agent_is_not_tracked() {
    let w = World::new();
    assert!(!w.router.is_tracked(&w.agent));
    assert_eq!(w.router.cert_of(&w.agent), None);
}

#[test]
fn enrollment_makes_an_agent_tracked() {
    let w = World::new();
    w.router.enroll(&w.agent, &CERT, &CAP);
    assert!(w.router.is_tracked(&w.agent));
    assert_eq!(w.router.cert_of(&w.agent), Some(CERT));
    assert_eq!(w.router.float_cap(&CERT), CAP);
    assert!(!w.router.is_halted(&CERT));
}

#[test]
fn enrolling_twice_is_refused_for_the_same_and_a_different_certificate() {
    let w = World::new();
    let registry_cert2: u64 = 2;
    w.router.enroll(&w.agent, &CERT, &CAP);

    assert!(w.router.try_enroll(&w.agent, &CERT, &CAP).is_err());
    assert!(w
        .router
        .try_enroll(&w.agent, &registry_cert2, &CAP)
        .is_err());
    // The original binding is intact.
    assert_eq!(w.router.cert_of(&w.agent), Some(CERT));
}

#[test]
fn tracked_transfers_accumulate_and_untracked_ones_do_not() {
    let w = World::new();
    let payee = w.addr();
    let outsider = w.addr();
    w.enrolled(20_0000000);

    w.router.transfer(&w.agent, &payee, &3_0000000);
    w.router.transfer(&w.agent, &payee, &2_0000000);
    assert_eq!(w.router.spent(&CERT), 5_0000000);

    // The payee is nobody's enrolled agent; its onward payments are not this
    // certificate's evidence.
    w.router.transfer(&payee, &outsider, &4_0000000);
    assert_eq!(w.router.spent(&CERT), 5_0000000);
    assert_eq!(w.router.balance(&outsider), 4_0000000);
    assert_fully_backed(&w);
}

/// **The $1 shuttle, against the real router.**
///
/// `spend-probe` proved the point in miniature; this proves it against the
/// contract the protocol will actually deploy. One dollar of float moves back
/// and forth between two addresses the operator controls. Every hop is a real,
/// authorized, correctly recorded payment. The counter climbs past the bound and
/// nothing leaves the pair.
///
/// This test exists to stop anyone later wiring `spent` directly to a payout.
#[test]
fn payment_router_reproduces_the_dollar_shuttle() {
    let w = World::new();
    let bound = 10_0000000; // $10
    let sink = w.addr(); // second address the operator controls
    let outsider = w.addr();

    w.enrolled(DOLLAR); // the entire float is one dollar

    let mut hops: i128 = 0;
    while w.router.spent(&CERT) <= bound {
        if hops % 2 == 0 {
            w.router.transfer(&w.agent, &sink, &DOLLAR);
        } else {
            w.router.transfer(&sink, &w.agent, &DOLLAR);
        }
        hops += 1;
    }

    // The `BoundExceeded` predicate is now true.
    assert!(w.router.spent(&CERT) > bound);
    assert_eq!(w.router.spent(&CERT), bound + DOLLAR); // $11 of "spend"

    // Only the agent's own outbound hops are metered — the return leg is the
    // untracked sink paying the agent — so 21 one-dollar payments book $11 of
    // spend against a $10 bound. At a $1,500 bound that is 3,001 payments:
    // minutes of work and pennies of fees.
    assert_eq!(hops, 2 * (bound / DOLLAR) + 1);
    assert_eq!(hops, 21);

    // Net flow is ~zero: the controlled pair holds exactly what it started with.
    assert_eq!(w.router.balance(&w.agent) + w.router.balance(&sink), DOLLAR);

    // Nobody outside the pair was touched.
    assert_eq!(w.router.balance(&outsider), 0);

    // And custody never moved: the whole episode cost gas and nothing else.
    assert_eq!(w.usdc_balance(&w.router_id), DOLLAR);
    assert_fully_backed(&w);
}

// ---------------------------------------------------------------------------
// Float cap (§6)
// ---------------------------------------------------------------------------

#[test]
fn deposit_at_the_cap_is_accepted_and_beyond_it_is_refused() {
    let w = World::new();
    w.router.enroll(&w.agent, &CERT, &CAP);

    w.router.deposit(&w.agent, &CAP);
    assert_eq!(w.router.float(&CERT), CAP);
    assert_eq!(w.router.balance(&w.agent), CAP);

    assert!(w.router.try_deposit(&w.agent, &DOLLAR).is_err());
    assert_eq!(w.router.float(&CERT), CAP);
    assert_eq!(w.router.balance(&w.agent), CAP);
    assert_fully_backed(&w);
}

#[test]
fn spending_frees_cap_headroom_and_the_operator_can_raise_the_cap() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(CAP);

    w.router.transfer(&w.agent, &payee, &10_0000000);
    assert_eq!(w.router.float(&CERT), CAP - 10_0000000);
    w.router.deposit(&w.agent, &10_0000000);
    assert_eq!(w.router.float(&CERT), CAP);

    w.router.set_float_cap(&CERT, &(CAP * 2));
    assert_eq!(w.router.float_cap(&CERT), CAP * 2);
    w.router.deposit(&w.agent, &CAP);
    assert_eq!(w.router.float(&CERT), CAP * 2);
}

#[test]
fn an_untracked_address_is_not_subject_to_any_cap() {
    let w = World::new();
    let stranger = w.addr();
    w.mint(&stranger, 1_000_0000000);
    w.router.deposit(&stranger, &500_0000000);
    assert_eq!(w.router.balance(&stranger), 500_0000000);
    assert_eq!(w.router.float(&CERT), 0);
}

// ---------------------------------------------------------------------------
// Kill switch (§6)
// ---------------------------------------------------------------------------

#[test]
fn operator_halt_stops_routing_and_the_agent_key_cannot_clear_it() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(10_0000000);

    w.router.transfer(&w.agent, &payee, &DOLLAR);

    // Only the operator's authorization is honoured from here on.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.operator,
        invoke: &MockAuthInvoke {
            contract: &w.router_id,
            fn_name: "halt",
            args: (CERT,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    w.router.halt(&CERT);
    assert!(w.router.is_halted(&CERT));

    w.env.mock_all_auths();

    // Routing is dead: no transfer, no withdraw, no burn.
    assert!(w.router.try_transfer(&w.agent, &payee, &DOLLAR).is_err());
    assert!(w.router.try_withdraw(&w.agent, &DOLLAR).is_err());
    assert!(w.router.try_burn(&w.agent, &DOLLAR).is_err());
    assert_eq!(w.router.balance(&w.agent), 9_0000000);
    assert_eq!(w.router.spent(&CERT), DOLLAR);

    // A thief holding the agent key cannot resume: `resume` requires the
    // certificate's operator, which the agent key is not. Auth is narrowed to
    // the agent alone to make that concrete.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.agent,
        invoke: &MockAuthInvoke {
            contract: &w.router_id,
            fn_name: "resume",
            args: (CERT,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.router.try_resume(&CERT).is_err());
    assert!(w.router.is_halted(&CERT));

    // Nor can the agent key raise its own cap to re-arm itself.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.agent,
        invoke: &MockAuthInvoke {
            contract: &w.router_id,
            fn_name: "set_float_cap",
            args: (CERT, CAP * 10).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.router.try_set_float_cap(&CERT, &(CAP * 10)).is_err());

    // The operator can resume, and routing returns exactly as it was.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.operator,
        invoke: &MockAuthInvoke {
            contract: &w.router_id,
            fn_name: "resume",
            args: (CERT,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    w.router.resume(&CERT);
    assert!(!w.router.is_halted(&CERT));

    w.env.mock_all_auths();
    w.router.transfer(&w.agent, &payee, &DOLLAR);
    assert_eq!(w.router.spent(&CERT), 2 * DOLLAR);
    assert_fully_backed(&w);
}

/// A halt must survive an allowance granted *before* it.
///
/// Otherwise a thief who has already `approve`d a second address they control
/// keeps draining the float through `transfer_from` after the operator has
/// halted — which defeats the kill switch being the fastest compromise response.
/// The allowance must be suspended, not destroyed: a halt/resume cycle may not
/// silently wipe legitimate standing approvals.
#[test]
fn halt_suspends_pre_existing_allowances_without_destroying_them() {
    let w = World::new();
    let spender = w.addr();
    let payee = w.addr();
    w.enrolled(10_0000000);

    // 1. The tracked agent approves a spender.
    w.router.approve(&w.agent, &spender, &6_0000000, &1_000u32);
    assert_eq!(w.router.allowance(&w.agent, &spender), 6_0000000);

    // 2. The operator halts.
    w.router.halt(&CERT);

    // 3. The spender's transfer_from is refused, and nothing moves.
    assert!(w
        .router
        .try_transfer_from(&spender, &w.agent, &payee, &4_0000000)
        .is_err());
    assert_eq!(w.router.balance(&w.agent), 10_0000000);
    assert_eq!(w.router.balance(&payee), 0);
    assert_eq!(w.router.spent(&CERT), 0);
    // The allowance is still on the books, untouched.
    assert_eq!(w.router.allowance(&w.agent, &spender), 6_0000000);

    // 4. The operator resumes.
    w.router.resume(&CERT);

    // 5. The very same transfer_from now succeeds.
    w.router
        .transfer_from(&spender, &w.agent, &payee, &4_0000000);
    assert_eq!(w.router.balance(&w.agent), 6_0000000);
    assert_eq!(w.router.balance(&payee), 4_0000000);
    assert_eq!(w.router.allowance(&w.agent, &spender), 2_0000000);
    assert_eq!(w.router.spent(&CERT), 4_0000000);
    assert_fully_backed(&w);
}

#[test]
fn halting_one_certificate_does_not_touch_another() {
    let w = World::new();
    let stranger = w.addr();
    let payee = w.addr();
    w.mint(&stranger, 100_0000000);
    w.router.deposit(&stranger, &10_0000000);
    w.enrolled(10_0000000);

    w.router.halt(&CERT);
    assert!(w.router.try_transfer(&w.agent, &payee, &DOLLAR).is_err());
    // An untracked holder is unaffected.
    w.router.transfer(&stranger, &payee, &DOLLAR);
    assert_eq!(w.router.balance(&payee), DOLLAR);
}

// ---------------------------------------------------------------------------
// Post-expiry counting (§7)
// ---------------------------------------------------------------------------

#[test]
fn post_expiry_payments_are_counted_separately_and_still_count_as_spend() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(20_0000000);

    // Before expiry: counted only as spend.
    w.router.transfer(&w.agent, &payee, &2_0000000);
    assert_eq!(w.router.spent(&CERT), 2_0000000);
    assert_eq!(w.router.post_expiry_spent(&CERT).total, 0);
    assert_eq!(w.router.post_expiry_spent(&CERT).count, 0);

    // At exactly expires_at the certificate has not yet expired.
    w.env.ledger().set_timestamp(EXPIRES_AT);
    w.router.transfer(&w.agent, &payee, &DOLLAR);
    assert_eq!(w.router.post_expiry_spent(&CERT).count, 0);

    // One second later it has.
    w.env.ledger().set_timestamp(EXPIRES_AT + 1);
    w.router.transfer(&w.agent, &payee, &DOLLAR);

    w.env.ledger().set_timestamp(EXPIRES_AT + 100_000);
    w.router.transfer(&w.agent, &payee, &5_0000000);

    w.env.ledger().set_timestamp(EXPIRES_AT + 200_000);
    w.router.transfer(&w.agent, &payee, &2_0000000);

    let pe = w.router.post_expiry_spent(&CERT);
    assert_eq!(pe.total, 8_0000000);
    assert_eq!(pe.count, 3);
    // The largest single post-expiry payment, and when it settled — the pair a
    // de-minimis floor is applied to.
    assert_eq!(pe.max_payment, 5_0000000);
    assert_eq!(pe.max_payment_at, EXPIRES_AT + 100_000);
    // The first post-expiry payment, which a grace window is measured against.
    assert_eq!(pe.first_at, EXPIRES_AT + 1);

    // Post-expiry flow is still flow: `spent` includes all of it.
    assert_eq!(w.router.spent(&CERT), 11_0000000);
}

/// §7 is deliberately *not* applied here. The router records; the predicate
/// judges. This test pins that division so a later change cannot quietly move
/// the grace window into the hot path.
#[test]
fn the_router_applies_neither_grace_window_nor_de_minimis_floor() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(10_0000000);

    // A one-dollar payment one second after expiry — inside any grace window and
    // below any floor — is still recorded, in full, without judgement.
    w.env.ledger().set_timestamp(EXPIRES_AT + 1);
    w.router.transfer(&w.agent, &payee, &DOLLAR);

    let pe = w.router.post_expiry_spent(&CERT);
    assert_eq!(pe.total, DOLLAR);
    assert_eq!(pe.max_payment, DOLLAR);
    assert_eq!(w.router.spent(&CERT), DOLLAR);
}

// ---------------------------------------------------------------------------
// Arithmetic and argument validation
// ---------------------------------------------------------------------------

#[test]
fn non_positive_amounts_are_refused_everywhere() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(10_0000000);

    assert!(w.router.try_transfer(&w.agent, &payee, &0).is_err());
    assert!(w.router.try_transfer(&w.agent, &payee, &-DOLLAR).is_err());
    assert!(w.router.try_deposit(&w.agent, &0).is_err());
    assert!(w.router.try_withdraw(&w.agent, &-1).is_err());
    assert!(w.router.try_burn(&w.agent, &0).is_err());
    assert!(w.router.try_enroll(&w.addr(), &CERT, &0).is_err());
    assert!(w.router.try_set_float_cap(&CERT, &-1).is_err());
    assert_eq!(w.router.spent(&CERT), 0);
}

/// The counter errors rather than wrapping. `overflow-checks` is on in release,
/// but the counter is the one number a hostile party most wants to roll over, so
/// it is checked explicitly and panics deliberately.
#[test]
fn the_spend_counter_errors_rather_than_wrapping() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(10_0000000);

    // Drive the counter to within one dollar of i128::MAX.
    w.env.as_contract(&w.router_id, || {
        w.env
            .storage()
            .persistent()
            .set(&DataKey::Spent(CERT), &(i128::MAX - 1));
    });

    assert!(w.router.try_transfer(&w.agent, &payee, &DOLLAR).is_err());
    w.env.as_contract(&w.router_id, || {
        let spent: i128 = w
            .env
            .storage()
            .persistent()
            .get(&DataKey::Spent(CERT))
            .unwrap();
        assert_eq!(spent, i128::MAX - 1, "no wrap");
    });
}

#[test]
#[should_panic(expected = "already_initialized")]
fn initialize_is_once_only() {
    let w = World::new();
    w.router.initialize(&w.addr(), &w.usdc);
}

#[test]
fn views_on_an_unenrolled_certificate_read_zero() {
    let w = World::new();
    assert_eq!(w.router.spent(&99), 0);
    assert_eq!(w.router.post_expiry_spent(&99).total, 0);
    assert_eq!(w.router.float(&99), 0);
    assert!(w.router.try_is_halted(&99).is_err());
}

// ---------------------------------------------------------------------------
// Clawback (§6) — recovering the float from a halted certificate without
// re-arming the thief.
// ---------------------------------------------------------------------------

/// The whole point: the operator gets their float back and the certificate
/// stays halted.
#[test]
fn operator_claws_back_the_float_and_routing_stays_halted() {
    let w = World::new();
    let payee = w.addr();
    w.enrolled(10_0000000);
    w.router.transfer(&w.agent, &payee, &DOLLAR);

    let agent_balance = w.router.balance(&w.agent);
    assert_eq!(agent_balance, 9_0000000);
    let operator_before = w.router.balance(&w.operator);
    let supply_before = w.router.total_supply();

    w.router.halt(&CERT);
    assert_eq!(w.router.clawback(&CERT, &w.agent), agent_balance);

    // The agent is empty and the operator is up by exactly what the agent held.
    assert_eq!(w.router.balance(&w.agent), 0);
    assert_eq!(
        w.router.balance(&w.operator),
        operator_before + agent_balance
    );

    // Custody never moved: no USDC left the router, and the invariant holds.
    assert_eq!(w.router.total_supply(), supply_before);
    assert_fully_backed(&w);

    // The certificate's float is gone with it.
    assert_eq!(w.router.float(&CERT), 0);

    // The meter is untouched — a recovery is not a payment.
    assert_eq!(w.router.spent(&CERT), DOLLAR);

    // The thief is not re-enabled. Routing is still dead even though the money
    // is safe, and it takes a deliberate `resume` to change that.
    assert!(w.router.is_halted(&CERT));
    assert!(w.router.try_transfer(&w.agent, &payee, &DOLLAR).is_err());
    assert!(w.router.try_withdraw(&w.agent, &DOLLAR).is_err());

    // And the operator can take the recovered money out for real: they are not
    // an enrolled agent of the certificate, so the halt does not gate them.
    let usdc_before = w.usdc_balance(&w.operator);
    w.router.withdraw(&w.operator, &agent_balance);
    assert_eq!(w.usdc_balance(&w.operator), usdc_before + agent_balance);
    assert_fully_backed(&w);
}

#[test]
fn clawback_while_not_halted_is_rejected() {
    let w = World::new();
    w.enrolled(10_0000000);
    assert!(!w.router.is_halted(&CERT));
    assert!(w.router.try_clawback(&CERT, &w.agent).is_err());
    assert_eq!(w.router.balance(&w.agent), 10_0000000);
}

/// The security property, asserted narrowly: a thief holding the **agent** key
/// cannot claw back to themselves. `mock_all_auths` would prove nothing here,
/// so authorization is narrowed to the agent alone.
#[test]
fn the_agent_key_cannot_call_clawback() {
    let w = World::new();
    w.enrolled(10_0000000);
    w.router.halt(&CERT);

    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.agent,
        invoke: &MockAuthInvoke {
            contract: &w.router_id,
            fn_name: "clawback",
            args: (CERT, w.agent.clone()).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.router.try_clawback(&CERT, &w.agent).is_err());

    w.env.mock_all_auths();
    assert_eq!(w.router.balance(&w.agent), 10_0000000);
}

#[test]
fn a_third_party_cannot_call_clawback() {
    let w = World::new();
    let stranger = w.addr();
    w.enrolled(10_0000000);
    w.router.halt(&CERT);

    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &w.router_id,
            fn_name: "clawback",
            args: (CERT, w.agent.clone()).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.router.try_clawback(&CERT, &w.agent).is_err());

    w.env.mock_all_auths();
    assert_eq!(w.router.balance(&w.agent), 10_0000000);
    assert_eq!(w.router.balance(&stranger), 0);
}

/// A second clawback is a no-op returning zero, not an error. During an
/// incident a retry must not fail loudly for having nothing left to do.
#[test]
fn a_second_clawback_on_an_empty_balance_is_a_no_op() {
    let w = World::new();
    w.enrolled(10_0000000);
    w.router.halt(&CERT);

    assert_eq!(w.router.clawback(&CERT, &w.agent), 10_0000000);
    let operator_after_first = w.router.balance(&w.operator);

    assert_eq!(w.router.clawback(&CERT, &w.agent), 0);
    assert_eq!(w.router.balance(&w.operator), operator_after_first);
    assert_eq!(w.router.balance(&w.agent), 0);
    assert_fully_backed(&w);
}

/// The agent has to belong to the certificate named in the call.
#[test]
fn clawback_refuses_an_agent_that_is_not_enrolled_on_this_certificate() {
    let w = World::new();
    let stranger = w.addr();
    w.enrolled(10_0000000);
    w.mint(&stranger, 100_0000000);
    w.router.deposit(&stranger, &50_0000000);
    w.router.halt(&CERT);

    // Untracked address holding a router balance: not reachable.
    assert!(w.router.try_clawback(&CERT, &stranger).is_err());
    assert_eq!(w.router.balance(&stranger), 50_0000000);
}
