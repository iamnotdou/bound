#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contracttype]
pub enum DataKey {
    ChallengeManager,
    Registry,
    Token,
    MinRegistrationStake,
    Stake(Address),
    // Timestamp until which this auditor's stake is bonded to a live attestation
    // and cannot be withdrawn. Set by the Registry on attest = cert.expires_at.
    LockedUntil(Address),
}

#[contract]
pub struct AuditorStaking;

#[contractimpl]
impl AuditorStaking {
    pub fn initialize(
        env: Env,
        challenge_manager: Address,
        registry: Address,
        token: Address,
        min_stake: i128,
    ) {
        if env.storage().instance().has(&DataKey::ChallengeManager) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::ChallengeManager, &challenge_manager);
        // Only the Registry may bond an auditor's stake to a certificate (on attest).
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::MinRegistrationStake, &min_stake);
    }

    // Stake USDC → otomatik olarak registered auditor olursun
    // Stake >= min_stake ise registered sayılırsın
    pub fn stake(env: Env, auditor: Address, amount: i128) {
        auditor.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &auditor,
            &env.current_contract_address(),
            &amount,
        );

        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Stake(auditor.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Stake(auditor), &(current + amount));
    }

    pub fn get_stake(env: Env, auditor: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Stake(auditor))
            .unwrap_or(0)
    }

    // Stake >= min_registration_stake ise registered sayılır — ayrı bir kayıt yok
    pub fn is_registered(env: Env, auditor: Address) -> bool {
        let min: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinRegistrationStake)
            .unwrap_or(0);
        let stake: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Stake(auditor))
            .unwrap_or(0);
        stake >= min
    }

    pub fn get_min_stake(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinRegistrationStake)
            .unwrap_or(0)
    }

    // Bond an auditor's stake to a live attestation until `until` (the cert's
    // expiry). Only the Registry may call this — it does so inside attest(), so
    // the moment an auditor vouches, their capital is locked and cannot be
    // pulled out from under the counterparty. Extends an existing lock, never
    // shortens it (an auditor backing several certs stays locked to the latest).
    pub fn lock(env: Env, auditor: Address, until: u64) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        registry.require_auth();

        let current: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedUntil(auditor.clone()))
            .unwrap_or(0);
        if until > current {
            env.storage().persistent().set(&DataKey::LockedUntil(auditor), &until);
        }
    }

    pub fn locked_until(env: Env, auditor: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::LockedUntil(auditor))
            .unwrap_or(0)
    }

    // Sadece ChallengeManager slash edebilir — yanlış attest ederse stake gider
    pub fn slash(env: Env, auditor: Address, recipient: Address, amount: i128) {
        let cm: Address = env.storage().instance().get(&DataKey::ChallengeManager).unwrap();
        cm.require_auth();

        let stake: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Stake(auditor.clone()))
            .unwrap_or(0);
        if amount > stake {
            panic!("slash_exceeds_stake");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &recipient,
            &amount,
        );

        env.storage()
            .persistent()
            .set(&DataKey::Stake(auditor), &(stake - amount));
    }

    // Temiz çıkış — stake'ini geri çek, artık registered değilsin.
    // Bir aktif attestation'a bağlıysa (cert süresi dolana dek) reddedilir —
    // denetçinin teminatı, vouch ettiği sürece counterparty'nin altından çekilemez.
    pub fn release(env: Env, auditor: Address) {
        auditor.require_auth();

        let locked_until: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedUntil(auditor.clone()))
            .unwrap_or(0);
        if env.ledger().timestamp() < locked_until {
            panic!("stake_locked");
        }

        let stake: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Stake(auditor.clone()))
            .unwrap_or(0);
        if stake == 0 {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &auditor,
            &stake,
        );

        env.storage()
            .persistent()
            .set(&DataKey::Stake(auditor), &0i128);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Env,
    };

    fn setup(env: &Env, min_stake: i128) -> (AuditorStakingClient, Address) {
        let cm = Address::generate(env);
        let registry = Address::generate(env);
        let token = Address::generate(env);
        let contract_id = env.register(AuditorStaking, ());
        let client = AuditorStakingClient::new(env, &contract_id);
        client.initialize(&cm, &registry, &token, &min_stake);
        (client, cm)
    }

    #[test]
    fn test_not_registered_with_zero_stake() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env, 500_0000000);
        let auditor = Address::generate(&env);
        assert!(!client.is_registered(&auditor));
    }

    #[test]
    fn test_registered_after_sufficient_stake() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env, 500_0000000);
        let auditor = Address::generate(&env);

        // Set stake directly to simulate a deposit
        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .set(&DataKey::Stake(auditor.clone()), &500_0000000i128);
        });

        assert!(client.is_registered(&auditor));
    }

    #[test]
    fn test_not_registered_below_min_stake() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env, 500_0000000);
        let auditor = Address::generate(&env);

        // Stake below minimum
        env.as_contract(&client.address, || {
            env.storage()
                .persistent()
                .set(&DataKey::Stake(auditor.clone()), &100_0000000i128); // $100 < $500 min
        });

        assert!(!client.is_registered(&auditor));
    }

    #[test]
    #[should_panic(expected = "slash_exceeds_stake")]
    fn test_slash_above_stake_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env, 500_0000000);
        let auditor = Address::generate(&env);
        let recipient = Address::generate(&env);
        client.slash(&auditor, &recipient, &100);
    }

    #[test]
    #[should_panic(expected = "stake_locked")]
    fn test_release_blocked_while_bonded_to_cert() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env, 500_0000000);
        let auditor = Address::generate(&env);

        // Auditor has stake, bonded to a cert that expires at t=5000.
        env.as_contract(&client.address, || {
            env.storage().persistent().set(&DataKey::Stake(auditor.clone()), &1_500_0000000i128);
            env.storage().persistent().set(&DataKey::LockedUntil(auditor.clone()), &5000u64);
        });
        env.ledger().set_timestamp(1000); // before expiry — stake must stay put
        client.release(&auditor); // → panic: stake_locked
    }

    #[test]
    fn test_release_allowed_after_lock_expires() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup(&env, 500_0000000);
        let auditor = Address::generate(&env);

        env.as_contract(&client.address, || {
            env.storage().persistent().set(&DataKey::LockedUntil(auditor.clone()), &5000u64);
        });
        // After the certificate's expiry the bond is lifted: release no longer
        // panics (zero stake → early return, but past the lock check).
        env.ledger().set_timestamp(6000);
        client.release(&auditor);
        assert_eq!(client.get_stake(&auditor), 0);
    }
}
