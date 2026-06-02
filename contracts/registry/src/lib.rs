#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, IntoVal, Symbol, Vec};

#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum CertStatus {
    Pending,   // operator yayınladı, auditor bekleniyor
    Verified,  // auditor onayladı
    Invalid,   // challenge ile iptal edildi
}

#[contracttype]
#[derive(Clone)]
pub struct Certificate {
    pub agent: Address,
    pub operator: Address,
    pub auditor: Option<Address>,
    pub bound: i128,
    pub reserve_amount: i128,
    pub auditor_stake_snapshot: i128,
    pub issued_at: u64,
    pub expires_at: u64,
    pub reserve_vault_contract: Address,
    pub auditor_staking_contract: Address,
    pub status: CertStatus,
}

#[contracttype]
#[derive(Clone)]
pub struct VerifyResult {
    pub valid: bool,
    pub status: CertStatus,
    pub bound: i128,
    pub reserve: i128,
    pub auditor_stake: i128,
    pub auditor: Option<Address>,
    pub expires_at: u64,
}

#[contracttype]
pub enum DataKey {
    ChallengeManager,
    AuditorStaking,
    CertCount,
    Certificate(u64),
    AgentCert(Address),
}

#[contract]
pub struct Registry;

#[contractimpl]
impl Registry {
    pub fn initialize(env: Env, challenge_manager: Address, auditor_staking: Address) {
        if env.storage().instance().has(&DataKey::ChallengeManager) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::ChallengeManager, &challenge_manager);
        env.storage().instance().set(&DataKey::AuditorStaking, &auditor_staking);
        env.storage().instance().set(&DataKey::CertCount, &0u64);
    }

    // Sadece operator imzaladı → PENDING olarak yayınlanır
    pub fn publish(
        env: Env,
        operator: Address,
        agent: Address,
        bound: i128,
        reserve_amount: i128,
        expires_at: u64,
        reserve_vault_contract: Address,
        auditor_staking_contract: Address,
    ) -> u64 {
        operator.require_auth();

        if bound <= 0 {
            panic!("invalid_bound");
        }
        if reserve_amount <= 0 {
            panic!("invalid_reserve");
        }
        if expires_at <= env.ledger().timestamp() {
            panic!("expiry_in_past");
        }

        let cert_count: u64 = env.storage().instance().get(&DataKey::CertCount).unwrap();
        let cert_id = cert_count + 1;

        let cert = Certificate {
            agent: agent.clone(),
            operator,
            auditor: None,
            bound,
            reserve_amount,
            auditor_stake_snapshot: 0,
            issued_at: env.ledger().timestamp(),
            expires_at,
            reserve_vault_contract,
            auditor_staking_contract,
            status: CertStatus::Pending,
        };

        env.storage().persistent().set(&DataKey::Certificate(cert_id), &cert);
        env.storage().persistent().set(&DataKey::AgentCert(agent), &cert_id);
        env.storage().instance().set(&DataKey::CertCount, &cert_id);

        cert_id
    }

    // Sadece registered auditor → VERIFIED
    pub fn attest(env: Env, auditor: Address, cert_id: u64) {
        auditor.require_auth();

        let auditor_staking: Address = env
            .storage()
            .instance()
            .get(&DataKey::AuditorStaking)
            .unwrap();

        // Cross-contract: AuditorStaking.is_registered(auditor)
        let is_registered: bool = env.invoke_contract(
            &auditor_staking,
            &Symbol::new(&env, "is_registered"),
            Vec::from_array(&env, [auditor.clone().into_val(&env)]),
        );

        if !is_registered {
            panic!("auditor_not_registered");
        }

        let mut cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");

        if cert.status != CertStatus::Pending {
            panic!("cert_not_pending");
        }

        // Cross-contract: AuditorStaking.get_stake(auditor)
        let stake_snapshot: i128 = env.invoke_contract(
            &auditor_staking,
            &Symbol::new(&env, "get_stake"),
            Vec::from_array(&env, [auditor.clone().into_val(&env)]),
        );

        cert.auditor = Some(auditor.clone());
        cert.auditor_stake_snapshot = stake_snapshot;
        cert.status = CertStatus::Verified;

        // Bond the auditor's stake to this certificate until it expires. From
        // this moment they cannot withdraw the capital they just vouched with —
        // the "skin in the game" is enforced on-chain, not merely advertised.
        env.invoke_contract::<()>(
            &auditor_staking,
            &Symbol::new(&env, "lock"),
            Vec::from_array(&env, [auditor.into_val(&env), cert.expires_at.into_val(&env)]),
        );

        env.storage().persistent().set(&DataKey::Certificate(cert_id), &cert);
    }

    pub fn verify(env: Env, agent: Address) -> VerifyResult {
        let cert_id: Option<u64> = env.storage().persistent().get(&DataKey::AgentCert(agent));

        let cert_id = match cert_id {
            None => {
                return VerifyResult {
                    valid: false,
                    status: CertStatus::Invalid,
                    bound: 0,
                    reserve: 0,
                    auditor_stake: 0,
                    auditor: None,
                    expires_at: 0,
                }
            }
            Some(id) => id,
        };

        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .unwrap();

        let expired = env.ledger().timestamp() > cert.expires_at;
        let valid = cert.status == CertStatus::Verified && !expired;

        VerifyResult {
            valid,
            status: cert.status,
            bound: cert.bound,
            reserve: cert.reserve_amount,
            auditor_stake: cert.auditor_stake_snapshot,
            auditor: cert.auditor,
            expires_at: cert.expires_at,
        }
    }

    pub fn get_certificate(env: Env, cert_id: u64) -> Certificate {
        env.storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found")
    }

    pub fn get_cert_id(env: Env, agent: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::AgentCert(agent))
            .expect("no_certificate_for_agent")
    }

    pub fn get_cert_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::CertCount).unwrap_or(0)
    }

    // ChallengeManager bunları cross-contract okur — slash kararı için gerekli alanlar
    pub fn get_cert_auditor(env: Env, cert_id: u64) -> Address {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.auditor.expect("cert_has_no_auditor")
    }

    pub fn get_cert_reserve(env: Env, cert_id: u64) -> i128 {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.reserve_amount
    }

    pub fn invalidate(env: Env, cert_id: u64) {
        let cm: Address = env.storage().instance().get(&DataKey::ChallengeManager).unwrap();
        cm.require_auth();

        let mut cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");

        cert.status = CertStatus::Invalid;
        env.storage().persistent().set(&DataKey::Certificate(cert_id), &cert);
    }
}

#[cfg(test)]
mod mock_staking_registered {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct MockStaking;

    #[contractimpl]
    impl MockStaking {
        pub fn is_registered(_env: Env, _auditor: Address) -> bool {
            true
        }
        pub fn get_stake(_env: Env, _auditor: Address) -> i128 {
            500_0000000
        }
        pub fn lock(_env: Env, _auditor: Address, _until: u64) {}
    }
}

#[cfg(test)]
mod mock_staking_unregistered {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct MockStaking;

    #[contractimpl]
    impl MockStaking {
        pub fn is_registered(_env: Env, _auditor: Address) -> bool {
            false
        }
        pub fn get_stake(_env: Env, _auditor: Address) -> i128 {
            0
        }
        pub fn lock(_env: Env, _auditor: Address, _until: u64) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Env,
    };

    fn setup_with_mock(env: &Env, registered: bool) -> (RegistryClient, Address) {
        let cm = Address::generate(env);

        let mock_staking_id = if registered {
            env.register(mock_staking_registered::MockStaking, ())
        } else {
            env.register(mock_staking_unregistered::MockStaking, ())
        };

        let registry_id = env.register(Registry, ());
        let client = RegistryClient::new(env, &registry_id);
        client.initialize(&cm, &mock_staking_id);
        (client, cm)
    }

    fn publish_cert(client: &RegistryClient, env: &Env, operator: &Address, agent: &Address) -> u64 {
        let rv = Address::generate(env);
        let ast = Address::generate(env);
        client.publish(
            operator,
            agent,
            &50_000_0000000i128,
            &10_000_0000000i128,
            &9_999_999u64,
            &rv,
            &ast,
        )
    }

    #[test]
    fn test_verify_unknown_agent_returns_invalid() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        let result = client.verify(&Address::generate(&env));
        assert!(!result.valid);
    }

    #[test]
    fn test_publish_creates_pending_cert() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        env.ledger().set_timestamp(1000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);

        let cert_id = publish_cert(&client, &env, &operator, &agent);

        let cert = client.get_certificate(&cert_id);
        assert_eq!(cert.status, CertStatus::Pending);
        assert!(cert.auditor.is_none());

        let result = client.verify(&agent);
        assert!(!result.valid);
        assert_eq!(result.status, CertStatus::Pending);
    }

    #[test]
    fn test_attest_makes_cert_verified() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        env.ledger().set_timestamp(1000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);

        let cert_id = publish_cert(&client, &env, &operator, &agent);
        client.attest(&auditor, &cert_id);

        let result = client.verify(&agent);
        assert!(result.valid);
        assert_eq!(result.status, CertStatus::Verified);
        assert_eq!(result.auditor_stake, 500_0000000);
        assert!(result.auditor.is_some());
    }

    #[test]
    #[should_panic(expected = "auditor_not_registered")]
    fn test_unregistered_auditor_cannot_attest() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, false); // unregistered mock

        env.ledger().set_timestamp(1000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);

        let cert_id = publish_cert(&client, &env, &operator, &agent);
        client.attest(&auditor, &cert_id);
    }

    #[test]
    #[should_panic(expected = "cert_not_pending")]
    fn test_cannot_attest_twice() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        env.ledger().set_timestamp(1000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);

        let cert_id = publish_cert(&client, &env, &operator, &agent);
        client.attest(&auditor, &cert_id);
        client.attest(&auditor, &cert_id); // ikinci kez → panic
    }

    #[test]
    fn test_expired_cert_not_valid() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        env.ledger().set_timestamp(1000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);
        let rv = Address::generate(&env);
        let ast = Address::generate(&env);

        let cert_id = client.publish(
            &operator, &agent,
            &50_000_0000000i128, &10_000_0000000i128,
            &2000u64,
            &rv, &ast,
        );

        client.attest(&auditor, &cert_id);
        assert!(client.verify(&agent).valid);

        env.ledger().set_timestamp(3000);
        assert!(!client.verify(&agent).valid);
    }

    #[test]
    fn test_invalidate_sets_invalid() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        env.ledger().set_timestamp(1000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);

        let cert_id = publish_cert(&client, &env, &operator, &agent);
        client.attest(&auditor, &cert_id);
        assert!(client.verify(&agent).valid);

        client.invalidate(&cert_id);
        assert!(!client.verify(&agent).valid);
        assert_eq!(client.get_certificate(&cert_id).status, CertStatus::Invalid);
    }

    #[test]
    #[should_panic(expected = "expiry_in_past")]
    fn test_publish_past_expiry_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);

        env.ledger().set_timestamp(5000);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let rv = Address::generate(&env);
        let ast = Address::generate(&env);

        client.publish(
            &operator, &agent,
            &50_000_0000000i128, &10_000_0000000i128,
            &1000u64,
            &rv, &ast,
        );
    }
}
