// USDC amounts are written as <dollars>_<7 decimals>, e.g. 50_000_0000000 is
// $50,000. Clippy reads that as inconsistent grouping and suggests
// 500_000_000_000, which is the same number with the dollar figure no longer
// legible. The grouping is deliberate.
#![allow(clippy::inconsistent_digit_grouping)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Env,
};

/// The Registry, as far as this contract is concerned, is one read:
/// `get_cert_settlement_deadline`. `release_allocation` consults it live so an
/// open claim window (DESIGN-V2 §1) can hold an allocation past the deadline
/// snapshotted at attestation.
#[soroban_sdk::contract]
pub struct MockRegistry;

#[soroban_sdk::contractimpl]
impl MockRegistry {
    pub fn set_deadline(env: Env, cert_id: u64, deadline: u64) {
        env.storage().persistent().set(&cert_id, &deadline);
    }
    pub fn get_cert_settlement_deadline(env: Env, cert_id: u64) -> u64 {
        env.storage().persistent().get(&cert_id).unwrap_or(0)
    }
}

struct Fixture<'a> {
    registry: MockRegistryClient<'a>,
    client: AuditorStakingClient<'a>,
    treasury: Address,
}

fn setup(env: &Env, min_stake: i128) -> Fixture<'_> {
    let cm = Address::generate(env);
    let registry = env.register(MockRegistry, ());
    let token = Address::generate(env);
    let treasury = Address::generate(env);
    let contract_id = env.register(AuditorStaking, ());
    let client = AuditorStakingClient::new(env, &contract_id);
    client.initialize(&cm, &registry, &token, &min_stake);
    Fixture {
        registry: MockRegistryClient::new(env, &registry),
        client,
        treasury,
    }
}

/// Credit stake directly, bypassing the token transfer: these tests are about
/// the allocation ledger, not custody. Custody is covered end to end by the
/// integration harness, which runs a real Stellar Asset Contract.
fn credit(env: &Env, client: &AuditorStakingClient, auditor: &Address, amount: i128) {
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .set(&DataKey::Stake(auditor.clone()), &amount);
    });
}

#[test]
fn test_not_registered_with_zero_stake() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 500_0000000);
    let auditor = Address::generate(&env);
    assert!(!f.client.is_registered(&auditor));
}

#[test]
fn test_registered_after_sufficient_stake() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 500_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 500_0000000);
    assert!(f.client.is_registered(&auditor));
    assert_eq!(f.client.get_free_stake(&auditor), 500_0000000);
    assert_eq!(f.client.get_allocated(&auditor), 0);
}

#[test]
fn test_not_registered_below_min_stake() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 500_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 100_0000000);
    assert!(!f.client.is_registered(&auditor));
}

/// Registration is judged on *free* stake. An auditor whose whole book is
/// already vouching for live certificates has nothing left to vouch with.
#[test]
fn test_fully_allocated_auditor_is_not_registered() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 500_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);

    f.client.allocate(&auditor, &1u64, &600_0000000, &5_000u64);
    assert_eq!(f.client.get_free_stake(&auditor), 0);
    assert_eq!(f.client.get_stake(&auditor), 600_0000000);
    assert!(!f.client.is_registered(&auditor));
}

#[test]
fn test_allocation_tracks_per_certificate() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 1_000_0000000);

    f.client.allocate(&auditor, &1u64, &300_0000000, &5_000u64);
    f.client.allocate(&auditor, &2u64, &200_0000000, &9_000u64);

    assert_eq!(f.client.get_allocation(&1u64), 300_0000000);
    assert_eq!(f.client.get_allocation(&2u64), 200_0000000);
    assert_eq!(f.client.get_allocated(&auditor), 500_0000000);
    assert_eq!(f.client.get_free_stake(&auditor), 500_0000000);
    assert_eq!(
        f.client.get_allocation_auditor(&1u64),
        Some(auditor.clone())
    );
    // LockedUntil is the latest deadline across the book, and never shortens.
    assert_eq!(f.client.locked_until(&auditor), 9_000);
    assert_eq!(f.client.allocation_unlock_at(&1u64), 5_000);
}

#[test]
#[should_panic(expected = "insufficient_free_stake")]
fn test_cannot_allocate_more_than_free_stake() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);

    f.client.allocate(&auditor, &1u64, &500_0000000, &5_000u64);
    // Only $100 free left; $200 must be refused.
    f.client.allocate(&auditor, &2u64, &200_0000000, &5_000u64);
}

#[test]
#[should_panic(expected = "allocation_below_minimum")]
fn test_allocation_below_min_stake_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 500_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &100_0000000, &5_000u64);
}

#[test]
#[should_panic(expected = "already_allocated")]
fn test_cannot_allocate_twice_to_one_certificate() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &100_0000000, &5_000u64);
    f.client.allocate(&auditor, &1u64, &100_0000000, &5_000u64);
}

#[test]
#[should_panic(expected = "slash_exceeds_allocation")]
fn test_slash_above_allocation_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &100_0000000, &5_000u64);
    f.client.slash_allocation(&1u64, &f.treasury, &200_0000000);
}

/// Retiring an allocation hands the unslashed remainder back as free stake.
/// Storage is emptied too, so nothing is stranded behind a dead certificate.
#[test]
fn test_retire_returns_the_remainder_to_free_stake() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &400_0000000, &5_000u64);
    assert_eq!(f.client.get_free_stake(&auditor), 200_0000000);

    f.client.retire_allocation(&1u64);

    assert_eq!(f.client.get_allocation(&1u64), 0);
    assert_eq!(f.client.get_allocation_auditor(&1u64), None);
    assert_eq!(f.client.get_allocated(&auditor), 0);
    assert_eq!(f.client.get_free_stake(&auditor), 600_0000000);
    assert_eq!(f.client.get_stake(&auditor), 600_0000000);
}

#[test]
#[should_panic(expected = "allocation_still_locked")]
fn test_release_allocation_blocked_before_settlement_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &400_0000000, &5_000u64);
    f.registry.set_deadline(&1u64, &5_000u64);

    env.ledger().set_timestamp(4_999);
    f.client.release_allocation(&1u64);
}

#[test]
fn test_release_allocation_allowed_after_settlement_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &400_0000000, &5_000u64);
    f.registry.set_deadline(&1u64, &5_000u64);

    env.ledger().set_timestamp(5_000);
    f.client.release_allocation(&1u64);
    assert_eq!(f.client.get_free_stake(&auditor), 600_0000000);
    assert_eq!(f.client.get_allocation(&1u64), 0);
}

/// `release` withdraws free stake only — allocated capital cannot leave.
#[test]
fn test_release_withdraws_free_stake_only() {
    let env = Env::default();
    env.mock_all_auths();
    let f = setup(&env, 100_0000000);
    let auditor = Address::generate(&env);
    credit(&env, &f.client, &auditor, 600_0000000);
    f.client.allocate(&auditor, &1u64, &600_0000000, &5_000u64);

    // Nothing free: the call is a no-op rather than a partial withdrawal.
    f.client.release(&auditor);
    assert_eq!(f.client.get_stake(&auditor), 600_0000000);
    assert_eq!(f.client.get_allocation(&1u64), 600_0000000);
}
