//! Unit tests (near-sdk unit-testing mocks). Every MUST behaviour and every
//! rejection path of the contract. Waterfall math vectors live in settle.rs;
//! here they are re-run through real contract state.

use super::*;
use near_sdk::test_utils::VMContextBuilder;
use near_sdk::testing_env;

const U: u128 = 1_000_000; // 1 USDC (6 dp)
const T0: u64 = 1_800_000_000; // seconds
const NS: u64 = 1_000_000_000;

fn a(s: &str) -> AccountId {
    s.parse().unwrap()
}

fn yocto() -> NearToken {
    NearToken::from_yoctonear(1)
}

fn config() -> Config {
    Config {
        claim_window_secs: 90,
        post_expiry_secs: 300,
        grace_secs: 60,
        min_stake: U128(10 * U),
        min_challenge_bond: U128(10 * U),
        challenger_fee_bps: 1000,
        de_minimis_bps: 10,
        hygiene_bounty: U128(10 * U),
        protocol_fee_bps: 1000,
    }
}

struct T {
    c: Bound,
    now: u64,
}

impl T {
    fn ctx(&self, pred: &str, deposit: NearToken) {
        let mut b = VMContextBuilder::new();
        b.current_account_id(a("bound.test"))
            .predecessor_account_id(a(pred))
            .signer_account_id(a(pred))
            .attached_deposit(deposit)
            .prepaid_gas(Gas::from_tgas(300))
            .block_timestamp(self.now * NS);
        testing_env!(b.build());
    }

    fn new() -> T {
        let mut b = VMContextBuilder::new();
        b.current_account_id(a("bound.test"))
            .predecessor_account_id(a("deployer.test"))
            .block_timestamp(T0 * NS);
        testing_env!(b.build());
        let c = Bound::new(
            a("owner.test"),
            a("usdc.test"),
            a("treasury.test"),
            a("arbiter.test"),
            config(),
        );
        T { c, now: T0 }
    }

    fn register(&mut self, who: &str) {
        self.ctx(who, STORAGE_DEPOSIT);
        self.c.storage_deposit(None);
    }

    /// Token-side deposit: the token calls ft_on_transfer.
    fn deposit(&mut self, who: &str, amount: u128, msg: &str) -> U128 {
        self.ctx("usdc.test", NearToken::from_yoctonear(0));
        match self.c.ft_on_transfer(a(who), U128(amount), msg.to_string()) {
            PromiseOrValue::Value(v) => v,
            _ => panic!("unexpected promise"),
        }
    }

    fn bal(&self, who: &str) -> u128 {
        self.c.balance_of(a(who)).0
    }

    fn cert0(&self) -> CertView {
        self.c.get_cert(0).unwrap()
    }

    fn publish(&mut self, allowlist: Option<Vec<AccountId>>) -> u64 {
        self.ctx("operator.test", PUBLISH_DEPOSIT);
        self.c.publish(
            a("agent.test"),
            U128(150 * U),
            U128(100 * U),
            self.now + 3600,
            a("auditor.test"),
            U128(20 * U),
            allowlist,
            Some(U128(3 * U)),
        )
    }

    fn accept(&mut self, id: u64) {
        self.ctx("agent.test", yocto());
        self.c.accept(id);
    }

    fn pay(&mut self, who: &str, to: &str, amount: u128) {
        self.ctx(who, NearToken::from_yoctonear(0));
        self.c.pay(0, a(to), U128(amount), Some("api call".into()));
    }

    fn harm(&mut self, who: &str, claimed: u128) -> u64 {
        self.ctx(who, CHALLENGE_DEPOSIT);
        self.c.challenge(
            0,
            Proof::Harm {
                claimed: U128(claimed),
                evidence_uri: "gate-invoices.json#1".into(),
            },
            None,
            U128(10 * U),
        )
    }

    fn resolve(&mut self, id: u64, proven: bool, harm: u128) {
        self.ctx("arbiter.test", yocto());
        self.c.resolve_by_arbiter(id, proven, U128(harm));
    }

    fn close(&mut self) -> Option<Settlement> {
        self.ctx("anyone.test", yocto());
        self.c.close_window(0)
    }
}

/// Fresh contract with every demo actor registered and funded internally.
fn funded() -> T {
    let mut t = T::new();
    for who in [
        "operator.test",
        "auditor.test",
        "service.test",
        "challenger.test",
        "attacker.test",
    ] {
        t.register(who);
    }
    t.deposit("operator.test", 1_000 * U, "");
    t.deposit("auditor.test", 500 * U, "{\"stake\":{}}");
    t.deposit("challenger.test", 50 * U, "");
    t.deposit("service.test", 10 * U, "");
    t
}

/// The demo certificate, Verified: bound 150, reserve 100, float 20,
/// allowlist [service], allocation 200, premium 3.
fn verified() -> T {
    let mut t = funded();
    let id = t.publish(Some(vec![a("service.test")]));
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.c.fund_float(id, U128(20 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(200 * U));
    t
}

// ---------------------------------------------------------------- init/config

#[test]
fn init_registers_treasury_and_arbiter() {
    let t = T::new();
    assert!(t.c.storage_balance_of(a("treasury.test")).is_some());
    assert!(t.c.storage_balance_of(a("arbiter.test")).is_some());
    assert_eq!(t.c.get_config().claim_window_secs, 90);
    assert_eq!(t.c.get_owner(), a("owner.test"));
    assert_eq!(t.c.get_accounts().token, a("usdc.test"));
}

#[test]
#[should_panic(expected = "invalid challenger_fee_bps")]
fn init_rejects_bad_fee() {
    let mut b = VMContextBuilder::new();
    b.current_account_id(a("bound.test"));
    testing_env!(b.build());
    let mut c = config();
    c.challenger_fee_bps = 10_001;
    Bound::new(a("o.test"), a("usdc.test"), a("t.test"), a("ar.test"), c);
}

// ---------------------------------------------------------------- storage

#[test]
fn storage_deposit_covers_usage() {
    let mut t = T::new();
    let before = env::storage_usage();
    t.register("someone-with-a-rather-long-account-name-for-testing.test");
    let used = env::storage_usage() - before;
    let cost = env::storage_byte_cost().saturating_mul(used as u128);
    assert!(cost <= STORAGE_DEPOSIT, "register: {used} bytes");

    // publish with a full 16-entry allowlist of long names
    let mut t = funded();
    let list: Vec<AccountId> = (0..16)
        .map(|i| {
            a(&format!(
                "recipient-number-{i:02}-with-a-long-name-padding-xx.test"
            ))
        })
        .collect();
    t.ctx("operator.test", PUBLISH_DEPOSIT);
    let before = env::storage_usage();
    t.c.publish(
        a("agent.test"),
        U128(150 * U),
        U128(100 * U),
        T0 + 3600,
        a("auditor.test"),
        U128(20 * U),
        Some(list),
        None,
    );
    let used = env::storage_usage() - before;
    assert!(env::storage_byte_cost().saturating_mul(used as u128 + 400) <= PUBLISH_DEPOSIT);

    // accept + first challenge stay within publish + challenge deposits
    let mut t = verified();
    let before = env::storage_usage();
    t.harm("service.test", 120 * U);
    let used = env::storage_usage() - before;
    assert!(env::storage_byte_cost().saturating_mul(used as u128) <= CHALLENGE_DEPOSIT);
}

#[test]
#[should_panic(expected = "StorageDepositTooSmall")]
fn storage_deposit_too_small_rejected() {
    let mut t = T::new();
    t.ctx("x.test", NearToken::from_millinear(1));
    t.c.storage_deposit(None);
}

#[test]
fn storage_deposit_twice_is_idempotent() {
    let mut t = T::new();
    t.register("x.test");
    t.register("x.test");
    assert!(t.c.storage_balance_of(a("x.test")).is_some());
}

// ---------------------------------------------------------------- deposits

#[test]
fn ft_on_transfer_credits_registered_sender() {
    let t = funded();
    assert_eq!(t.bal("operator.test"), 1_000 * U);
    assert_eq!(t.c.get_stake(a("auditor.test")).free.0, 500 * U);
    assert_eq!(t.bal("auditor.test"), 0);
}

#[test]
fn ft_on_transfer_refunds_unregistered_beneficiary() {
    let mut t = T::new();
    assert_eq!(t.deposit("nobody.test", 5 * U, "").0, 5 * U);
    assert_eq!(t.bal("nobody.test"), 0);
    assert!(t.c.storage_balance_of(a("nobody.test")).is_none());
}

#[test]
fn ft_on_transfer_credit_for_other_account() {
    let mut t = funded();
    let r = t.deposit(
        "operator.test",
        7 * U,
        "{\"credit\":{\"account_id\":\"service.test\"}}",
    );
    assert_eq!(r.0, 0);
    assert_eq!(t.bal("service.test"), 17 * U);
}

#[test]
#[should_panic(expected = "UnknownToken")]
fn ft_on_transfer_from_other_token_rejected() {
    let mut t = funded();
    t.ctx("fake-usdc.test", NearToken::from_yoctonear(0));
    let _ =
        t.c.ft_on_transfer(a("operator.test"), U128(U), String::new());
}

#[test]
#[should_panic(expected = "InvalidMsg")]
fn ft_on_transfer_bad_msg_panics_so_token_refunds() {
    let mut t = funded();
    t.deposit("operator.test", U, "{not json");
}

#[test]
fn ft_on_transfer_reserve_and_float_actions() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.deposit(
        "operator.test",
        100 * U,
        &format!("{{\"reserve\":{{\"cert_id\":{id}}}}}"),
    );
    t.deposit(
        "operator.test",
        20 * U,
        &format!("{{\"float\":{{\"cert_id\":{id}}}}}"),
    );
    let c = t.cert0();
    assert_eq!(c.reserve_locked.0, 100 * U);
    assert_eq!(c.float.0, 20 * U);
}

// ---------------------------------------------------------------- withdraw

#[test]
fn withdraw_debits_before_promise() {
    let mut t = funded();
    t.ctx("challenger.test", yocto());
    let _ = t.c.withdraw(U128(20 * U));
    assert_eq!(t.bal("challenger.test"), 30 * U);
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn withdraw_without_yocto_rejected() {
    let mut t = funded();
    t.ctx("challenger.test", NearToken::from_yoctonear(0));
    let _ = t.c.withdraw(U128(U));
}

#[test]
#[should_panic(expected = "InsufficientBalance")]
fn withdraw_more_than_balance_rejected() {
    let mut t = funded();
    t.ctx("challenger.test", yocto());
    let _ = t.c.withdraw(U128(51 * U));
}

#[test]
#[should_panic(expected = "NotEnoughGas")]
fn withdraw_with_low_gas_rejected() {
    let mut t = funded();
    let mut b = VMContextBuilder::new();
    b.current_account_id(a("bound.test"))
        .predecessor_account_id(a("challenger.test"))
        .attached_deposit(yocto())
        .prepaid_gas(Gas::from_tgas(20))
        .block_timestamp(t.now * NS);
    testing_env!(b.build());
    let _ = t.c.withdraw(U128(U));
}

// ---------------------------------------------------------------- staking

#[test]
fn stake_and_unstake_free_only() {
    let mut t = verified();
    let s = t.c.get_stake(a("auditor.test"));
    assert_eq!(s.free.0, 300 * U);
    assert_eq!(s.allocated.0, 200 * U);
    t.ctx("auditor.test", yocto());
    t.c.unstake(U128(300 * U));
    assert_eq!(t.c.get_stake(a("auditor.test")).free.0, 0);
    // premium 3: 2.7 to auditor at attest, plus unstaked 300
    assert_eq!(t.bal("auditor.test"), 300 * U + 2_700_000);
}

#[test]
#[should_panic(expected = "InsufficientFreeStake")]
fn unstake_allocated_rejected() {
    let mut t = verified();
    t.ctx("auditor.test", yocto());
    t.c.unstake(U128(301 * U));
}

#[test]
#[should_panic(expected = "InsufficientBalance")]
fn stake_more_than_balance_rejected() {
    let mut t = funded();
    t.ctx("challenger.test", yocto());
    t.c.stake(U128(51 * U));
}

// ---------------------------------------------------------------- certificates

#[test]
fn verify_returns_verified_view() {
    let t = verified();
    let v = t.c.verify(a("agent.test")).expect("cert");
    assert_eq!(v.status, "Verified");
    assert_eq!(v.bound.0, 150 * U);
    assert_eq!(v.reserve_locked.0, 100 * U);
    assert_eq!(v.allocation.0, 200 * U);
    assert_eq!(v.float.0, 20 * U);
    assert_eq!(v.premium.0, 0);
    assert_eq!(v.settlement_deadline, T0 + 3600 + 300);
    assert!(t.c.verify(a("stranger.test")).is_none());
    assert_eq!(t.c.list_certs(None, None).len(), 1);
    assert_eq!(t.c.cert_count(), 1);
}

#[test]
fn premium_paid_at_attest_90_10() {
    let t = verified();
    assert_eq!(t.bal("auditor.test"), 2_700_000);
    assert_eq!(t.bal("treasury.test"), 300_000);
    // operator: 1000 - 3 premium - 100 reserve - 20 float
    assert_eq!(t.bal("operator.test"), 877 * U);
}

#[test]
fn verify_reads_expired_after_expiry() {
    let mut t = verified();
    t.now += 3600;
    t.ctx("anyone.test", NearToken::from_yoctonear(0));
    assert_eq!(t.c.verify(a("agent.test")).unwrap().status, "Expired");
}

#[test]
#[should_panic(expected = "NotAccepted")]
fn attest_refuses_without_accept() {
    let mut t = funded();
    let id = t.publish(None);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(200 * U));
}

#[test]
#[should_panic(expected = "ReserveUnfunded")]
fn attest_refuses_unfunded_reserve() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(99 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(200 * U));
}

#[test]
#[should_panic(expected = "NotAuditor")]
fn attest_by_other_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.ctx("challenger.test", yocto());
    t.c.attest(id, U128(200 * U));
}

#[test]
#[should_panic(expected = "AllocationBelowMinStake")]
fn attest_below_min_stake_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(U));
}

#[test]
#[should_panic(expected = "InsufficientFreeStake")]
fn attest_above_free_stake_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(501 * U));
}

#[test]
#[should_panic(expected = "NotAgent")]
fn accept_by_other_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.ctx("operator.test", yocto());
    t.c.accept(id);
}

#[test]
#[should_panic(expected = "exactly 1 yoctoNEAR")]
fn accept_without_yocto_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.ctx("agent.test", NearToken::from_yoctonear(0));
    t.c.accept(id);
}

#[test]
#[should_panic(expected = "AgentHasLiveCert")]
fn second_live_cert_rejected() {
    let mut t = verified();
    let id = t.publish(None);
    t.accept(id);
}

#[test]
fn pending_cert_does_not_block_agent() {
    let mut t = funded();
    // a griefer publishes certs naming the agent; they stay Pending
    let _ = t.publish(None);
    let id2 = t.publish(None);
    t.accept(id2);
    assert_eq!(t.c.verify(a("agent.test")).unwrap().cert_id, id2);
}

#[test]
#[should_panic(expected = "PublishDepositTooSmall")]
fn publish_without_deposit_rejected() {
    let mut t = funded();
    t.ctx("operator.test", yocto());
    t.c.publish(
        a("agent.test"),
        U128(1),
        U128(1),
        T0 + 10,
        a("auditor.test"),
        U128(0),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "InvalidFloatCap")]
fn publish_float_cap_above_bound_rejected() {
    let mut t = funded();
    t.ctx("operator.test", PUBLISH_DEPOSIT);
    t.c.publish(
        a("agent.test"),
        U128(10),
        U128(10),
        T0 + 10,
        a("auditor.test"),
        U128(11),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "InvalidExpiry")]
fn publish_expired_rejected() {
    let mut t = funded();
    t.ctx("operator.test", PUBLISH_DEPOSIT);
    t.c.publish(
        a("agent.test"),
        U128(10),
        U128(10),
        T0,
        a("auditor.test"),
        U128(1),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "InvalidAuditor")]
fn publish_self_audit_rejected() {
    let mut t = funded();
    t.ctx("operator.test", PUBLISH_DEPOSIT);
    t.c.publish(
        a("agent.test"),
        U128(10),
        U128(10),
        T0 + 10,
        a("operator.test"),
        U128(1),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "InvalidAllowlist")]
fn publish_oversized_allowlist_rejected() {
    let mut t = funded();
    let list: Vec<AccountId> = (0..17).map(|i| a(&format!("r{i}.test"))).collect();
    t.ctx("operator.test", PUBLISH_DEPOSIT);
    t.c.publish(
        a("agent.test"),
        U128(10),
        U128(10),
        T0 + 10,
        a("auditor.test"),
        U128(1),
        Some(list),
        None,
    );
}

#[test]
#[should_panic(expected = "NotRegistered")]
fn publish_by_unregistered_operator_rejected() {
    let mut t = funded();
    t.ctx("stranger.test", PUBLISH_DEPOSIT);
    t.c.publish(
        a("agent.test"),
        U128(10),
        U128(10),
        T0 + 10,
        a("auditor.test"),
        U128(1),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "NotOperator")]
fn fund_reserve_by_other_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.ctx("challenger.test", yocto());
    t.c.fund_reserve(id, U128(U));
}

#[test]
#[should_panic(expected = "FloatCapExceeded")]
fn fund_float_above_cap_rejected() {
    let mut t = verified();
    t.ctx("operator.test", yocto());
    t.c.fund_float(0, U128(1));
}

// ---------------------------------------------------------------- router

#[test]
fn spent_increments() {
    let mut t = verified();
    t.pay("agent.test", "service.test", 5 * U);
    t.pay("agent.test", "service.test", 2 * U);
    let v = t.cert0();
    assert_eq!(v.spent.0, 7 * U);
    assert_eq!(v.float.0, 13 * U);
    assert_eq!(v.pay_count, 2);
    assert_eq!(t.bal("service.test"), 17 * U);
}

#[test]
#[should_panic(expected = "FloatExceeded")]
fn pay_over_float_rejected() {
    let mut t = verified();
    t.pay("agent.test", "service.test", 5 * U);
    t.pay("agent.test", "service.test", 50 * U);
}

#[test]
#[should_panic(expected = "CertHalted")]
fn pay_when_halted_rejected() {
    let mut t = verified();
    t.ctx("operator.test", yocto());
    t.c.halt(0);
    t.pay("agent.test", "service.test", U);
}

#[test]
fn resume_after_halt_allows_pay() {
    let mut t = verified();
    t.ctx("operator.test", yocto());
    t.c.halt(0);
    t.c.resume(0);
    t.pay("agent.test", "service.test", U);
    assert_eq!(t.cert0().spent.0, U);
}

#[test]
#[should_panic(expected = "NotOperator")]
fn halt_by_agent_rejected() {
    let mut t = verified();
    t.ctx("agent.test", yocto());
    t.c.halt(0);
}

#[test]
#[should_panic(expected = "NotAgent")]
fn pay_by_non_agent_rejected() {
    let mut t = verified();
    t.pay("operator.test", "service.test", U);
}

#[test]
#[should_panic(expected = "RecipientNotAllowed")]
fn pay_to_unlisted_recipient_rejected() {
    let mut t = verified();
    t.pay("agent.test", "attacker.test", 10 * U);
}

#[test]
#[should_panic(expected = "RecipientNotRegistered")]
fn pay_to_unregistered_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.c.fund_float(id, U128(20 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(200 * U));
    t.pay("agent.test", "fresh-unregistered.test", U);
}

#[test]
#[should_panic(expected = "InvalidRecipient")]
fn pay_to_self_rejected() {
    let mut t = verified();
    t.pay("agent.test", "agent.test", U);
}

#[test]
#[should_panic(expected = "CertFrozen")]
fn pay_when_frozen_rejected() {
    let mut t = verified();
    t.harm("service.test", 120 * U);
    t.pay("agent.test", "service.test", U);
}

#[test]
#[should_panic(expected = "CertNotVerified")]
fn pay_before_attest_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.accept(id);
    t.pay("agent.test", "service.test", U);
}

#[test]
#[should_panic(expected = "MemoTooLong")]
fn pay_memo_too_long_rejected() {
    let mut t = verified();
    t.ctx("agent.test", NearToken::from_yoctonear(0));
    t.c.pay(0, a("service.test"), U128(U), Some("x".repeat(65)));
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn pay_zero_rejected() {
    let mut t = verified();
    t.pay("agent.test", "service.test", 0);
}

#[test]
fn clawback_returns_float() {
    let mut t = verified();
    t.ctx("operator.test", yocto());
    assert_eq!(t.c.clawback(0).0, 20 * U);
    assert_eq!(t.cert0().float.0, 0);
    assert_eq!(t.bal("operator.test"), 897 * U);
}

#[test]
fn post_expiry_spend_is_metered_separately() {
    let mut t = verified();
    t.now = T0 + 3600 + 61; // after expiry + grace
    t.pay("agent.test", "service.test", 2 * U);
    let v = t.cert0();
    assert_eq!(v.spent.0, 2 * U);
    assert_eq!(v.post_expiry_spent.0, 2 * U);
}

// ---------------------------------------------------------------- challenges

#[test]
fn false_insufficient_reserve_rejected_bond_to_treasury() {
    let mut t = verified();
    let treasury_before = t.bal("treasury.test");
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    let id =
        t.c.challenge(0, Proof::InsufficientReserve, None, U128(10 * U));
    let ch = t.c.get_challenge(id).unwrap();
    assert_eq!(ch.verdict, Verdict::Rejected);
    assert_eq!(t.bal("challenger.test"), 40 * U);
    assert_eq!(t.bal("treasury.test"), treasury_before + 10 * U);
    assert!(t.c.get_window(0).is_none(), "no window opened");
    assert_eq!(t.cert0().status, "Verified");
}

#[test]
fn false_bound_exceeded_rejected() {
    let mut t = verified();
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    let id = t.c.challenge(0, Proof::BoundExceeded, None, U128(10 * U));
    assert_eq!(t.c.get_challenge(id).unwrap().verdict, Verdict::Rejected);
}

#[test]
fn false_expired_spend_rejected() {
    let mut t = verified();
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    let id = t.c.challenge(0, Proof::ExpiredSpend, None, U128(10 * U));
    assert_eq!(t.c.get_challenge(id).unwrap().verdict, Verdict::Rejected);
}

#[test]
#[should_panic(expected = "BondTooSmall")]
fn challenge_bond_too_small_rejected() {
    let mut t = verified();
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    t.c.challenge(0, Proof::BoundExceeded, None, U128(U));
}

#[test]
#[should_panic(expected = "ChallengeDepositTooSmall")]
fn challenge_without_deposit_rejected() {
    let mut t = verified();
    t.ctx("challenger.test", yocto());
    t.c.challenge(0, Proof::BoundExceeded, None, U128(10 * U));
}

#[test]
#[should_panic(expected = "NotChallengeable")]
fn challenge_unattested_rejected() {
    let mut t = funded();
    let id = t.publish(None);
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    t.c.challenge(id, Proof::BoundExceeded, None, U128(10 * U));
}

#[test]
#[should_panic(expected = "SettlementDeadlinePassed")]
fn challenge_after_deadline_rejected() {
    let mut t = verified();
    t.now = T0 + 3600 + 301;
    t.harm("service.test", U);
}

#[test]
#[should_panic(expected = "NotRegistered")]
fn challenge_naming_unregistered_victim_rejected() {
    let mut t = verified();
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    t.c.challenge(
        0,
        Proof::Harm {
            claimed: U128(U),
            evidence_uri: String::new(),
        },
        Some(a("ghost.test")),
        U128(10 * U),
    );
}

#[test]
fn harm_claim_opens_window_and_freezes() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    let w = t.c.get_window(0).unwrap();
    assert_eq!(w.closes_at, T0 + 90);
    assert_eq!(w.claims, vec![id]);
    assert_eq!(t.cert0().status, "Challenged");
    assert_eq!(t.bal("service.test"), 0);
}

#[test]
#[should_panic(expected = "NotArbiter")]
fn resolve_by_non_arbiter_rejected() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    t.ctx("service.test", yocto());
    t.c.resolve_by_arbiter(id, true, U128(120 * U));
}

#[test]
#[should_panic(expected = "InvalidHarm")]
fn resolve_harm_above_claimed_rejected() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    t.resolve(id, true, 121 * U);
}

#[test]
#[should_panic(expected = "AlreadyResolved")]
fn resolve_twice_rejected() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    t.resolve(id, true, 120 * U);
    t.resolve(id, true, 100 * U);
}

#[test]
#[should_panic(expected = "NotArbitrable")]
fn resolve_predicate_claim_rejected() {
    let mut t = verified();
    t.now = T0 + 3600 + 61;
    t.pay("agent.test", "service.test", 2 * U); // post-expiry spend > 0.15
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    let id = t.c.challenge(0, Proof::ExpiredSpend, None, U128(10 * U));
    t.resolve(id, true, U);
}

#[test]
#[should_panic(expected = "WindowFull")]
fn seventeenth_claim_rejected() {
    let mut t = verified();
    t.deposit("challenger.test", 500 * U, "");
    for _ in 0..17 {
        t.harm("challenger.test", U);
    }
}

#[test]
fn sixteen_claims_fit() {
    let mut t = verified();
    t.deposit("challenger.test", 500 * U, "");
    for _ in 0..16 {
        t.harm("challenger.test", U);
    }
    assert_eq!(t.c.get_window(0).unwrap().claims.len(), 16);
}

// ---------------------------------------------------------------- settlement

#[test]
#[should_panic(expected = "WindowOpen")]
fn early_close_refused() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    t.resolve(id, true, 120 * U);
    t.now += 89;
    t.close();
}

#[test]
#[should_panic(expected = "NoOpenWindow")]
fn close_without_window_refused() {
    let mut t = verified();
    t.close();
}

/// The testnet demo vector, end to end through contract state:
/// reserve 100, allocation 200, harm 120.
#[test]
fn demo_vector_harm_above_reserve() {
    let mut t = verified();
    t.pay("agent.test", "service.test", 5 * U);
    let treasury0 = t.bal("treasury.test");
    let id = t.harm("service.test", 120 * U);
    t.resolve(id, true, 120 * U);
    t.now += 90;
    let s = t.close().expect("settled");
    assert_eq!(s.mode, "fraud");
    assert_eq!(s.payable.0, 120 * U);
    // 100 from the reserve + 20 from the auditor's slash: made whole.
    assert_eq!(s.victim_paid.0, 120 * U);
    assert_eq!(s.fee_paid.0, 0);
    assert_eq!(s.slash.0, 20 * U);
    assert_eq!(s.slash_to_victims.0, 20 * U);
    assert_eq!(s.to_treasury.0, 0);
    assert_eq!(s.auditor_returned.0, 180 * U);
    // service: 10 minted - 10 bond + 5 paid + 120 compensation + 10 bond back
    assert_eq!(t.bal("service.test"), 135 * U);
    assert_eq!(t.bal("treasury.test"), treasury0);
    // The unslashed 180 stays allocated until the settlement deadline.
    let st = t.c.get_stake(a("auditor.test"));
    assert_eq!(st.allocated.0, 180 * U);
    assert_eq!(st.free.0, 300 * U);
    let v = t.c.verify(a("agent.test")).unwrap();
    assert_eq!(v.status, "Invalid");
    assert_eq!(v.reserve_locked.0, 0);
    assert_eq!(v.float.0, 0);
    // remaining float (15) went back to the operator
    assert_eq!(t.bal("operator.test"), 877 * U + 15 * U);
    assert_eq!(t.c.get_challenge(id).unwrap().verdict, Verdict::Upheld);
}

#[test]
fn harm_below_reserve_no_slash_fee_from_reserve() {
    let mut t = verified();
    let id = t.harm("service.test", 60 * U);
    t.resolve(id, true, 60 * U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, 60 * U);
    assert_eq!(s.fee_paid.0, 6 * U);
    assert_eq!(s.slash.0, 0);
    assert_eq!(s.auditor_returned.0, 200 * U);
    // 0 + 60 + 6 fee + 10 bond back
    assert_eq!(t.bal("service.test"), 76 * U);
    // 34 left in the reserve for the operator to release at the deadline
    assert_eq!(t.cert0().reserve_locked.0, 34 * U);
    t.now = t.cert0().settlement_deadline;
    t.ctx("operator.test", yocto());
    assert_eq!(t.c.release_reserve(0).0, 34 * U); // float went back at settlement
}

#[test]
fn harm_above_reserve_plus_allocation_capped() {
    let mut t = verified();
    let id = t.harm("service.test", 1_000 * U);
    t.resolve(id, true, 1_000 * U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.payable.0, 300 * U);
    assert_eq!(s.victim_paid.0, 300 * U); // capped by coverage = 100 + 200
    assert_eq!(s.slash.0, 200 * U);
    assert_eq!(s.auditor_returned.0, 0);
}

#[test]
fn two_victims_pro_rata() {
    let mut t = verified();
    t.deposit("challenger.test", U, "");
    let a1 = t.harm("service.test", 30 * U);
    let a2 = t.harm("challenger.test", 90 * U);
    t.resolve(a1, true, 30 * U);
    t.resolve(a2, true, 90 * U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, 120 * U);
    // service: 10 - 10 + 25 reserve + 5 slash + 10
    assert_eq!(t.bal("service.test"), 40 * U);
    // challenger: 51 - 10 + 75 reserve + 15 slash + 10
    assert_eq!(t.bal("challenger.test"), 141 * U);
}

#[test]
fn arbiter_rejected_claim_forfeits_bond_window_reopens_cert() {
    let mut t = verified();
    let id = t.harm("service.test", 50 * U);
    let treasury0 = t.bal("treasury.test");
    t.resolve(id, false, 0);
    // Settled at rejection: bond to the treasury, slot freed.
    assert_eq!(t.bal("treasury.test"), treasury0 + 10 * U);
    assert!(t.c.get_window(0).unwrap().claims.is_empty());
    t.now += 90;
    assert!(t.close().is_none());
    assert_eq!(t.bal("treasury.test"), treasury0 + 10 * U);
    assert_eq!(t.c.get_challenge(id).unwrap().verdict, Verdict::Rejected);
    assert_eq!(t.cert0().status, "Verified");
    t.pay("agent.test", "service.test", U); // unfrozen
}

#[test]
fn unadjudicated_claim_forfeits_half_the_bond() {
    let mut t = verified();
    let treasury0 = t.bal("treasury.test");
    let id = t.harm("service.test", 50 * U);
    t.now += 90 + 30; // window + arbiter grace
    assert!(t.close().is_none());
    assert_eq!(t.bal("service.test"), 5 * U);
    assert_eq!(t.bal("treasury.test"), treasury0 + 5 * U);
    assert_eq!(
        t.c.get_challenge(id).unwrap().verdict,
        Verdict::Unadjudicated
    );
}

#[test]
fn bound_exceeded_settles_in_hygiene_mode() {
    let mut t = verified();
    // refill the float past the bound: 8 x 20 = 160 > 150
    for _ in 0..8 {
        t.pay("agent.test", "service.test", 20 * U);
        t.ctx("operator.test", yocto());
        t.c.fund_float(0, U128(20 * U));
    }
    assert!(t.cert0().spent.0 > 150 * U);
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    let id = t.c.challenge(0, Proof::BoundExceeded, None, U128(10 * U));
    assert_eq!(t.c.get_challenge(id).unwrap().verdict, Verdict::Pending);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.mode, "hygiene");
    assert_eq!(s.hygiene_bounty_paid.0, 10 * U);
    assert_eq!(s.slash.0, 0);
    assert_eq!(s.auditor_returned.0, 200 * U);
    // 50 - 10 bond + 10 bounty + 10 bond back
    assert_eq!(t.bal("challenger.test"), 60 * U);
    assert_eq!(t.cert0().status, "Invalid");
}

#[test]
fn expired_spend_settles_in_hygiene_mode() {
    let mut t = verified();
    t.now = T0 + 3600 + 61;
    t.pay("agent.test", "service.test", 2 * U);
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    let id = t.c.challenge(0, Proof::ExpiredSpend, None, U128(10 * U));
    assert_eq!(t.c.get_challenge(id).unwrap().verdict, Verdict::Pending);
    t.now += 90;
    assert_eq!(t.close().unwrap().mode, "hygiene");
}

#[test]
#[should_panic(expected = "NotChallengeable")]
fn settled_cert_refuses_new_predicate_challenge() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    t.resolve(id, true, 120 * U);
    t.now += 90;
    t.close();
    // A Harm claim is still accepted until the deadline (see the C1
    // regressions); a trustless predicate is not.
    t.harm("challenger.test", U);
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    t.c.challenge(0, Proof::InsufficientReserve, None, U128(10 * U));
}

#[test]
#[should_panic(expected = "CertNotVerified")]
fn settled_cert_refuses_pay() {
    let mut t = verified();
    let id = t.harm("service.test", 120 * U);
    t.resolve(id, true, 120 * U);
    t.now += 90;
    t.close();
    t.pay("agent.test", "service.test", U);
}

// ---------------------------------------------------------------- release

#[test]
#[should_panic(expected = "Locked")]
fn release_reserve_before_deadline_rejected() {
    let mut t = verified();
    t.now = T0 + 3600 + 299;
    t.ctx("operator.test", yocto());
    t.c.release_reserve(0);
}

#[test]
fn release_after_deadline_returns_collateral() {
    let mut t = verified();
    t.now = T0 + 3600 + 300;
    t.ctx("operator.test", yocto());
    assert_eq!(t.c.release_reserve(0).0, 120 * U);
    t.ctx("auditor.test", yocto());
    assert_eq!(t.c.release_allocation(0).0, 200 * U);
    assert_eq!(t.c.get_stake(a("auditor.test")).free.0, 500 * U);
    assert_eq!(t.cert0().status, "Released");
}

#[test]
#[should_panic(expected = "CertFrozen")]
fn release_while_frozen_rejected() {
    let mut t = verified();
    t.harm("service.test", 120 * U);
    t.now = T0 + 3600 + 300;
    t.ctx("operator.test", yocto());
    t.c.release_reserve(0);
}

#[test]
#[should_panic(expected = "Locked")]
fn release_allocation_before_deadline_rejected() {
    let mut t = verified();
    t.ctx("auditor.test", yocto());
    t.c.release_allocation(0);
}

#[test]
fn release_unattested_refunds_premium() {
    let mut t = funded();
    let id = t.publish(None);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    assert_eq!(t.c.release_reserve(id).0, 103 * U); // reserve + premium
    assert_eq!(t.bal("operator.test"), 1_000 * U);
    assert_eq!(t.cert0().status, "Released");
}

#[test]
#[should_panic(expected = "NothingToRelease")]
fn release_twice_rejected() {
    let mut t = verified();
    t.now = T0 + 3600 + 300;
    t.ctx("operator.test", yocto());
    t.c.release_reserve(0);
    t.c.release_reserve(0);
}

#[test]
fn list_certs_paginates() {
    let mut t = funded();
    for _ in 0..3 {
        t.publish(None);
    }
    assert_eq!(t.c.list_certs(Some(1), Some(1))[0].cert_id, 1);
    assert_eq!(t.c.list_certs(Some(2), Some(100)).len(), 1);
    assert!(t.c.list_certs(Some(9), None).is_empty());
}

#[test]
fn pay_emits_nep297_event() {
    let mut t = verified();
    t.pay("agent.test", "service.test", 5 * U);
    let logs = near_sdk::test_utils::get_logs();
    let ev = logs
        .iter()
        .find(|l| l.starts_with("EVENT_JSON:") && l.contains("\"event\":\"pay\""))
        .expect("pay event");
    let v: serde_json::Value = serde_json::from_str(&ev["EVENT_JSON:".len()..]).unwrap();
    assert_eq!(v["standard"], "bound");
    assert_eq!(v["data"][0]["to"], "service.test");
    assert_eq!(v["data"][0]["amount"], "5000000");
}

// ------------------------------------------------ regressions (judge review)

fn harm_on(t: &mut T, who: &str, cert_id: u64) -> u64 {
    t.ctx(who, CHALLENGE_DEPOSIT);
    t.c.challenge(
        cert_id,
        Proof::Harm {
            claimed: U128(U),
            evidence_uri: "x".into(),
        },
        None,
        U128(10 * U),
    )
}

fn panics(f: impl FnOnce()) -> String {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    match r {
        Ok(_) => String::new(),
        Err(e) => e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default(),
    }
}

/// B1: an id above u32::MAX never aliases cert 0 (challenge, pay, views).
#[test]
fn alias_cert_id_rejected_everywhere() {
    let mut t = verified();
    let alias: u64 = 1u64 << 32;
    let e = panics(|| {
        harm_on(&mut t, "challenger.test", alias);
    });
    assert!(e.contains("NoSuchCert"), "{e}");
    let e = panics(|| {
        t.ctx("agent.test", NearToken::from_yoctonear(0));
        t.c.pay(alias, a("service.test"), U128(U), None);
    });
    assert!(e.contains("NoSuchCert"), "{e}");
    let e = panics(|| {
        t.ctx("anyone.test", yocto());
        t.c.close_window(alias);
    });
    assert!(e.contains("NoSuchCert"), "{e}");
    assert!(t.c.get_cert(alias).is_none());
    assert!(t.c.get_window(alias).is_none());
    assert!(t.c.get_challenge(1u64 << 32).is_none());
}

/// B1 exploit path: collateral cannot leave while the real window is open,
/// and an upheld victim is paid.
#[test]
fn collateral_stays_locked_while_window_open() {
    let mut t = verified();
    t.now = T0 + 3600 + 300 - 10; // 10 s before the settlement deadline
    let real = t.harm("service.test", 50 * U);
    t.resolve(real, true, 50 * U);
    t.now += 100; // past the deadline, window still open
    let e = panics(|| {
        t.ctx("auditor.test", yocto());
        t.c.release_allocation(0);
    });
    assert!(e.contains("CertFrozen"), "{e}");
    let e = panics(|| {
        t.ctx("operator.test", yocto());
        t.c.release_reserve(0);
    });
    assert!(e.contains("CertFrozen"), "{e}");
    let before = t.bal("service.test");
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, 50 * U);
    assert_eq!(t.bal("service.test") - before, 50 * U + 5 * U + 10 * U);
}

/// B1: a settled window cannot be settled again; a settled certificate is
/// settled again only through a new window of Harm claims.
#[test]
fn close_window_refuses_settled_cert() {
    let mut t = verified();
    let id = t.harm("service.test", 50 * U);
    t.resolve(id, true, 50 * U);
    t.now += 90;
    t.close().unwrap();
    let e = panics(|| {
        t.close();
    });
    assert!(e.contains("NoOpenWindow"), "{e}");
    t.now = t.cert0().settlement_deadline;
    t.ctx("operator.test", yocto());
    t.c.release_reserve(0);
    let e = panics(|| {
        t.close();
    });
    assert!(e.contains("NotChallengeable"), "{e}");
}

/// B1: the 16-claim cap holds per certificate.
#[test]
fn window_cap_holds() {
    let mut t = verified();
    t.deposit("challenger.test", 200 * U, "");
    for _ in 0..16 {
        harm_on(&mut t, "challenger.test", 0);
    }
    let e = panics(|| {
        harm_on(&mut t, "challenger.test", 0);
    });
    assert!(e.contains("WindowFull"), "{e}");
}

/// M3: operator, agent and auditor cannot challenge their own certificate.
#[test]
fn conflicted_challenger_rejected() {
    for who in ["operator.test", "agent.test", "auditor.test"] {
        let mut t = verified();
        if who == "agent.test" {
            t.register("agent.test");
            t.deposit("agent.test", 20 * U, "");
        }
        let e = panics(|| {
            harm_on(&mut t, who, 0);
        });
        assert!(e.contains("ConflictedChallenger"), "{who}: {e}");
    }
}

/// M3: repeated freezing costs half a bond each time.
#[test]
fn grief_freeze_costs_bond() {
    let mut t = verified();
    let b0 = t.bal("challenger.test");
    for _ in 0..3 {
        harm_on(&mut t, "challenger.test", 0);
        t.now += 90 + 30;
        assert!(t.close().is_none());
        t.now += 1;
    }
    assert_eq!(b0 - t.bal("challenger.test"), 15 * U);
}

/// M3: a claim late in a window extends it to a third of a window.
#[test]
fn late_claim_extends_window() {
    let mut t = verified();
    harm_on(&mut t, "challenger.test", 0);
    let w0 = t.c.get_window(0).unwrap().closes_at;
    t.now += 80; // 10 s left
    t.harm("service.test", 10 * U);
    let w1 = t.c.get_window(0).unwrap().closes_at;
    assert_eq!(w1, t.now + 30);
    assert!(w1 > w0);
}

/// M3: nobody can close at `closes_at` to front-run an undecided Harm claim;
/// the arbiter can still resolve during the grace period.
#[test]
fn arbiter_grace_blocks_front_run() {
    let mut t = verified();
    let id = t.harm("service.test", 50 * U);
    t.now += 90;
    let e = panics(|| {
        t.close();
    });
    assert!(e.contains("ArbiterGrace"), "{e}");
    t.resolve(id, true, 50 * U);
    let s = t.close().unwrap(); // decided: closable now
    assert_eq!(s.victim_paid.0, 50 * U);
}

/// m1: an escrowed premium comes back even if nothing was ever funded.
#[test]
fn premium_refunded_when_reserve_never_funded() {
    let mut t = funded();
    let before = t.bal("operator.test");
    let id = t.publish(None);
    t.ctx("operator.test", yocto());
    assert_eq!(t.c.release_reserve(id).0, 3 * U);
    assert_eq!(t.bal("operator.test"), before);
}

/// m2: no payments after the settlement deadline.
#[test]
fn pay_after_deadline_rejected() {
    let mut t = verified();
    t.now = T0 + 3600 + 300 + 1;
    let e = panics(|| t.pay("agent.test", "service.test", U));
    assert!(e.contains("SettlementDeadlinePassed"), "{e}");
}

/// M1: coverage = reserve_locked + allocation is exposed by verify.
#[test]
fn verify_reports_coverage() {
    let t = verified();
    let v = t.c.verify(a("agent.test")).unwrap();
    assert_eq!(v.coverage.0, 300 * U);
}

// ------------------------------------------- regressions (review 2026-11-14)

/// minor: a halted cert does not read as Verified, and `deal` is false
/// whenever the agent cannot pay right now.
#[test]
fn halted_or_empty_float_is_not_a_deal() {
    let mut t = verified();
    assert!(t.cert0().deal);
    t.ctx("operator.test", yocto());
    t.c.halt(0);
    let v = t.c.verify(a("agent.test")).unwrap();
    assert_eq!(v.status, "Halted");
    assert!(!v.deal);
    t.ctx("operator.test", yocto());
    t.c.resume(0);
    assert_eq!(t.cert0().status, "Verified");
    assert!(t.cert0().deal);
    t.ctx("operator.test", yocto());
    t.c.clawback(0);
    assert_eq!(t.cert0().status, "Verified");
    assert!(!t.cert0().deal);
}

/// minor: only the agent's current cert (the one verify reports) can pay;
/// an older cert reads Superseded.
#[test]
fn superseded_cert_cannot_pay() {
    let mut t = verified();
    t.now = T0 + 3600 + 10; // old cert expired, deadline not reached
    let id = t.publish(None);
    t.accept(id);
    assert_eq!(t.c.verify(a("agent.test")).unwrap().cert_id, id);
    assert_eq!(t.cert0().status, "Superseded");
    assert!(!t.cert0().deal);
    let e = panics(|| t.pay("agent.test", "service.test", U));
    assert!(e.contains("NotCurrentCert"), "{e}");
}

/// Demo cert without an allowlist, so the agent can pay an operator sybil.
fn open_cert(t: &mut T) {
    let id = t.publish(None);
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(100 * U));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(200 * U));
}

fn harm_for(t: &mut T, who: &str, victim: Option<&str>, claimed: u128) -> u64 {
    t.ctx(who, CHALLENGE_DEPOSIT);
    t.c.challenge(
        0,
        Proof::Harm {
            claimed: U128(claimed),
            evidence_uri: "x".into(),
        },
        victim.map(a),
        U128(10 * U),
    )
}

/// C1: a self-inflicted hygiene settlement does not end the operator's
/// liability. Collateral stays locked until the deadline and a later victim
/// is still paid from it.
#[test]
fn c1_hygiene_settlement_keeps_liability_until_deadline() {
    let mut t = funded();
    t.register("sybil.test");
    t.register("victim.test");
    t.deposit("sybil.test", 10 * U, "");
    t.deposit("victim.test", 10 * U, "");
    open_cert(&mut t);
    // Shuttle 8 x 20 = 160 > bound 150 through the sybil (gross flow).
    for _ in 0..8 {
        t.ctx("operator.test", yocto());
        t.c.fund_float(0, U128(20 * U));
        t.pay("agent.test", "sybil.test", 20 * U);
    }
    t.ctx("sybil.test", CHALLENGE_DEPOSIT);
    t.c.challenge(0, Proof::BoundExceeded, None, U128(10 * U));
    t.now += 91;
    let s = t.close().unwrap();
    assert_eq!(s.mode, "hygiene");
    let v = t.cert0();
    assert_eq!(v.status, "Invalid");
    assert!(v.halted);
    assert!(!v.deal);
    assert!(t.now + 3000 < v.settlement_deadline);
    // Nothing leaves before the deadline.
    let e = panics(|| {
        t.ctx("operator.test", yocto());
        t.c.release_reserve(0);
    });
    assert!(e.contains("Locked"), "{e}");
    let e = panics(|| {
        t.ctx("auditor.test", yocto());
        t.c.release_allocation(0);
    });
    assert!(e.contains("Locked"), "{e}");
    assert_eq!(t.c.get_stake(a("auditor.test")).allocated.0, 200 * U);
    // A later victim is still heard and paid.
    let id = harm_for(&mut t, "victim.test", None, 50 * U);
    t.resolve(id, true, 50 * U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.mode, "fraud");
    assert_eq!(s.victim_paid.0, 50 * U);
    // 10 - 10 bond + 50 + 5 fee + 10 bond back
    assert_eq!(t.bal("victim.test"), 65 * U);
    // Trustless predicates cannot drain a settled cert again.
    let e = panics(|| {
        t.ctx("sybil.test", CHALLENGE_DEPOSIT);
        t.c.challenge(0, Proof::BoundExceeded, None, U128(10 * U));
    });
    assert!(e.contains("NotChallengeable"), "{e}");
    // After the deadline the rest comes back: 100 - 10 bounty - 50 - 5 fee.
    t.now = t.cert0().settlement_deadline;
    t.ctx("operator.test", yocto());
    assert_eq!(t.c.release_reserve(0).0, 35 * U);
    t.ctx("auditor.test", yocto());
    assert_eq!(t.c.release_allocation(0).0, 200 * U);
    assert_eq!(t.cert0().status, "Released");
}

/// C1, fraud variant: settling a tiny real harm does not free the rest of
/// the collateral before the deadline.
#[test]
fn c1_tiny_fraud_settlement_keeps_liability_until_deadline() {
    let mut t = verified();
    t.register("victim.test");
    t.deposit("victim.test", 10 * U, "");
    let id = t.harm("service.test", U);
    t.resolve(id, true, U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, U);
    assert_eq!(t.cert0().reserve_locked.0, 98_900_000);
    assert_eq!(t.cert0().allocation.0, 200 * U);
    let e = panics(|| {
        t.ctx("auditor.test", yocto());
        t.c.release_allocation(0);
    });
    assert!(e.contains("Locked"), "{e}");
    let id = harm_for(&mut t, "victim.test", None, 120 * U);
    t.resolve(id, true, 120 * U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, 120 * U);
    assert_eq!(s.slash.0, 21_100_000);
    assert_eq!(s.auditor_returned.0, 178_900_000);
    let st = t.c.get_stake(a("auditor.test"));
    assert_eq!(st.allocated.0, 178_900_000);
    t.now = t.cert0().settlement_deadline;
    t.ctx("auditor.test", yocto());
    assert_eq!(t.c.release_allocation(0).0, 178_900_000);
    assert_eq!(t.c.get_stake(a("auditor.test")).free.0, 478_900_000);
}

/// C2: an arbiter-rejected claim frees its slot at once (bond to the
/// treasury), and a victim can still file after the deadline while the
/// window opened before it is open.
#[test]
fn c2_rejected_claims_free_slots_and_late_victim_is_heard() {
    let mut t = verified();
    t.register("victim.test");
    // 20: the refused filing below is not rolled back by the unit-test mock.
    t.deposit("victim.test", 20 * U, "");
    t.deposit("attacker.test", 160 * U, "");
    let deadline = t.cert0().settlement_deadline;
    t.now = deadline - 5;
    for _ in 0..16 {
        t.harm("attacker.test", 1);
    }
    let e = panics(|| {
        harm_for(&mut t, "victim.test", None, 100 * U);
    });
    assert!(e.contains("WindowFull"), "{e}");
    let treasury0 = t.bal("treasury.test");
    for id in 0..16u64 {
        t.resolve(id, false, 0);
    }
    assert_eq!(t.bal("treasury.test"), treasury0 + 160 * U);
    assert!(t.c.get_window(0).unwrap().claims.is_empty());
    t.now = deadline + 10; // past the deadline, window still open
    let id = harm_for(&mut t, "victim.test", None, 100 * U);
    t.resolve(id, true, 100 * U);
    t.now = t.c.get_window(0).unwrap().closes_at;
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, 100 * U);
    // No new window can open after the deadline.
    let e = panics(|| {
        harm_for(&mut t, "victim.test", None, U);
    });
    assert!(e.contains("SettlementDeadlinePassed"), "{e}");
}

/// C2: true trustless claims cannot squat a window either: one per kind.
#[test]
fn c2_duplicate_trustless_claim_refused() {
    let mut t = verified();
    t.now = T0 + 3600 + 61;
    t.pay("agent.test", "service.test", 2 * U);
    t.ctx("challenger.test", CHALLENGE_DEPOSIT);
    t.c.challenge(0, Proof::ExpiredSpend, None, U128(10 * U));
    let e = panics(|| {
        t.ctx("challenger.test", CHALLENGE_DEPOSIT);
        t.c.challenge(0, Proof::ExpiredSpend, None, U128(10 * U));
    });
    assert!(e.contains("DuplicateClaim"), "{e}");
}

/// minor: at now == deadline the collateral is releasable and no claim is
/// accepted, so a claim never lands on released collateral.
#[test]
fn deadline_boundary_is_consistent() {
    let mut t = verified();
    let deadline = t.cert0().settlement_deadline;
    t.now = deadline;
    t.ctx("auditor.test", yocto());
    assert_eq!(t.c.release_allocation(0).0, 200 * U);
    let e = panics(|| {
        t.harm("challenger.test", 150 * U);
    });
    assert!(e.contains("SettlementDeadlinePassed"), "{e}");
    let e = panics(|| t.pay("agent.test", "service.test", U));
    assert!(e.contains("SettlementDeadlinePassed"), "{e}");
}

/// minor: a huge arbiter harm is capped at coverage instead of overflowing
/// the waterfall and freezing the cert forever.
#[test]
fn huge_harm_capped_at_coverage() {
    let mut t = verified();
    let id = t.harm("challenger.test", u128::MAX / 2);
    t.resolve(id, true, u128::MAX / 2);
    assert_eq!(t.c.get_challenge(id).unwrap().harm.0, 300 * U);
    t.now += 200;
    let s = t.close().unwrap();
    assert_eq!(s.victim_paid.0, 300 * U);
}

/// minor: the victim cannot be the operator, the agent or the auditor.
#[test]
fn conflicted_victim_rejected() {
    for who in ["operator.test", "agent.test", "auditor.test"] {
        let mut t = verified();
        let e = panics(|| {
            harm_for(&mut t, "challenger.test", Some(who), U);
        });
        assert!(e.contains("ConflictedVictim"), "{who}: {e}");
    }
}

fn transfers_to(who: &str) -> Vec<NearToken> {
    near_sdk::test_utils::get_created_receipts()
        .into_iter()
        .filter(|r| r.receiver_id == a(who))
        .flat_map(|r| r.actions)
        .filter_map(|x| match x {
            near_sdk::mock::MockAction::Transfer { deposit, .. } => Some(deposit),
            _ => None,
        })
        .collect()
}

/// minor: excess NEAR attached to publish and challenge is refunded.
#[test]
fn excess_deposit_refunded() {
    let mut t = funded();
    t.ctx("operator.test", NearToken::from_near(1));
    t.c.publish(
        a("agent.test"),
        U128(150 * U),
        U128(100 * U),
        t.now + 3600,
        a("auditor.test"),
        U128(20 * U),
        None,
        None,
    );
    assert_eq!(
        transfers_to("operator.test"),
        vec![NearToken::from_millinear(950)]
    );
    let mut t = verified();
    t.ctx("challenger.test", NearToken::from_millinear(110));
    t.c.challenge(
        0,
        Proof::Harm {
            claimed: U128(U),
            evidence_uri: "x".into(),
        },
        None,
        U128(10 * U),
    );
    assert_eq!(
        transfers_to("challenger.test"),
        vec![NearToken::from_millinear(100)]
    );
}

/// minor: coverage above ~1.8e19 base units used to overflow `pool * w` in
/// distribute and freeze the certificate; the 256-bit mul_div settles it.
#[test]
fn large_coverage_settles_without_overflow() {
    let mut t = funded();
    let big: u128 = 20_000_000_000_000_000_000; // 2e19 base units
    t.deposit("operator.test", big, "");
    let id = t.publish(Some(vec![a("service.test")]));
    t.accept(id);
    t.ctx("operator.test", yocto());
    t.c.fund_reserve(id, U128(big));
    t.ctx("auditor.test", yocto());
    t.c.attest(id, U128(200 * U));
    let h = t.harm("challenger.test", big);
    t.resolve(h, true, big);
    t.now += 200;
    let s = t.close().unwrap();
    assert_eq!(s.mode, "fraud");
    assert_eq!(s.victim_paid.0, big);
    assert_eq!(s.slash.0, 0);
}

/// minor: a window that moves no funds on a drained Invalid cert does not
/// overwrite the original fraud settlement with an empty hygiene record.
#[test]
fn empty_second_settlement_keeps_the_first() {
    let mut t = verified();
    let id = t.harm("service.test", 300 * U);
    t.resolve(id, true, 300 * U);
    t.now += 90;
    let s = t.close().unwrap();
    assert_eq!(s.mode, "fraud");
    assert_eq!(s.victim_paid.0, 300 * U);
    assert_eq!(t.cert0().reserve_locked.0 + t.cert0().allocation.0, 0);
    let id = t.harm("service.test", 50 * U);
    t.resolve(id, true, 50 * U);
    assert_eq!(t.c.get_challenge(id).unwrap().harm.0, 0);
    t.now += 90;
    let s2 = t.close().unwrap();
    assert_eq!(s2.mode, "hygiene");
    assert_eq!(s2.hygiene_bounty_paid.0, 0);
    let kept = t.cert0().settlement.unwrap();
    assert_eq!(kept.mode, "fraud");
    assert_eq!(kept.victim_paid.0, 300 * U);
}

/// Round 2: after the deadline, rejecting and refiling a junk claim cannot
/// push the window past one claim window after the deadline.
#[test]
fn post_deadline_extension_is_capped() {
    let mut t = verified();
    t.deposit("attacker.test", 10_000 * U, "");
    let deadline = t.cert0().settlement_deadline;
    let cap = deadline + 90;
    t.now = deadline - 1;
    let mut id = t.harm("attacker.test", 1);
    let mut refiled = 0;
    loop {
        t.resolve(id, false, 0);
        let w = t.c.get_window(0).unwrap();
        assert!(w.closes_at <= cap, "closes_at {} > cap {cap}", w.closes_at);
        t.now = w.closes_at - 1;
        id = t.harm("attacker.test", 1);
        refiled += 1;
        if t.c.get_window(0).unwrap().closes_at == cap {
            break;
        }
        assert!(refiled < 10, "extension never reached the cap");
    }
    // Once the capped window closes, no more claims and the collateral frees.
    t.now = cap;
    let e = panics(|| {
        t.harm("attacker.test", 1);
    });
    assert!(e.contains("SettlementDeadlinePassed"), "{e}");
    // The arbiter still gets its grace after the cap.
    let e = panics(|| {
        t.close();
    });
    assert!(e.contains("ArbiterGrace"), "{e}");
    t.resolve(id, false, 0);
    assert!(t.close().is_none());
    t.ctx("auditor.test", yocto());
    assert_eq!(t.c.release_allocation(0).0, 200 * U);
}

/// Round 2: a claim filed just before the deadline still gets the full
/// extension when it fits under the cap.
#[test]
fn claim_just_before_deadline_still_extends() {
    let mut t = verified();
    let deadline = t.cert0().settlement_deadline;
    harm_on(&mut t, "challenger.test", 0);
    t.now = t.c.get_window(0).unwrap().closes_at - 1;
    assert!(t.now < deadline);
    t.harm("service.test", 10 * U);
    assert_eq!(t.c.get_window(0).unwrap().closes_at, t.now + 30);
}

/// Round 2: a settlement that drew the whole reserve still lets the operator
/// mark the cert Released after the deadline (amount 0), and only once.
#[test]
fn release_reserve_marks_released_when_empty() {
    let mut t = verified();
    let id = t.harm("service.test", 10_000 * U);
    t.resolve(id, true, 10_000 * U);
    t.now += 90;
    t.close().unwrap();
    let c = t.cert0();
    assert_eq!(c.reserve_locked.0 + c.float.0, 0);
    assert_eq!(c.status, "Invalid");
    t.now = c.settlement_deadline;
    t.ctx("operator.test", yocto());
    assert_eq!(t.c.release_reserve(0).0, 0);
    assert_eq!(t.cert0().status, "Released");
    let e = panics(|| {
        t.ctx("operator.test", yocto());
        t.c.release_reserve(0);
    });
    assert!(e.contains("NothingToRelease"), "{e}");
}

