#![no_std]
// `publish` takes 8 arguments, one over clippy's default threshold. The argument
// list of a `pub fn` in a #[contractimpl] block is the contract's on-chain ABI:
// changing it would change the generated bindings and force a redeploy to a new
// address. The lint has to be silenced at crate level rather than on the impl
// block, because #[contractimpl] re-emits the signature as sibling items that an
// item-level allow does not cover.
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, IntoVal, Symbol, Vec};

/// How long after a certificate expires a challenge may still be filed against
/// it — and therefore how long the money backing it must stay put.
///
/// A proof about post-expiry activity only becomes provable *after* expiry. If
/// the operator's reserve and the auditor's allocation both unlocked at
/// `expires_at`, such a proof would settle against an empty pot every single
/// time. Both the ReserveVault and AuditorStaking read the deadline below, so
/// this constant is the one place the window is defined.
pub const CHALLENGE_WINDOW_SECONDS: u64 = 7 * 24 * 60 * 60;

#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum CertStatus {
    Pending,  // operator yayınladı, auditor bekleniyor
    Verified, // auditor onayladı
    Invalid,  // challenge ile iptal edildi
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
        env.storage()
            .instance()
            .set(&DataKey::ChallengeManager, &challenge_manager);
        env.storage()
            .instance()
            .set(&DataKey::AuditorStaking, &auditor_staking);
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

        env.storage()
            .persistent()
            .set(&DataKey::Certificate(cert_id), &cert);
        env.storage()
            .persistent()
            .set(&DataKey::AgentCert(agent), &cert_id);
        env.storage().instance().set(&DataKey::CertCount, &cert_id);

        cert_id
    }

    /// Sadece registered auditor → VERIFIED.
    ///
    /// `allocation` is how much of the auditor's **free** stake stands behind
    /// this one certificate. It is a parameter rather than a derived number
    /// because the alternatives are both worse: allocating the auditor's whole
    /// stake reproduces v1, where one bad certificate destroys an entire book;
    /// allocating a protocol-fixed amount would price every certificate the
    /// same regardless of the bound it backs. The auditor is the party pricing
    /// the risk, so the auditor names the number — and AuditorStaking enforces
    /// the two limits that matter: it must be at least `min_stake` (the same
    /// floor `is_registered` uses, now applied per certificate rather than per
    /// auditor) and it cannot exceed the auditor's free stake.
    ///
    /// The allocation is locked until this certificate's settlement deadline,
    /// not its expiry — see `CHALLENGE_WINDOW_SECONDS`.
    pub fn attest(env: Env, auditor: Address, cert_id: u64, allocation: i128) {
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

        // The snapshot is now the amount actually at risk for *this*
        // certificate, not the auditor's whole book. A counterparty reading
        // `verify` sees the collateral that would actually be drawn on if this
        // certificate turns out to be a lie.
        cert.auditor = Some(auditor.clone());
        cert.auditor_stake_snapshot = allocation;
        cert.status = CertStatus::Verified;

        let unlock_at = cert.expires_at.saturating_add(CHALLENGE_WINDOW_SECONDS);

        // Allocate that slice of the auditor's stake to this certificate. From
        // this moment they cannot withdraw the capital they just vouched with —
        // the "skin in the game" is enforced on-chain, not merely advertised.
        env.invoke_contract::<()>(
            &auditor_staking,
            &Symbol::new(&env, "allocate"),
            Vec::from_array(
                &env,
                [
                    auditor.into_val(&env),
                    cert_id.into_val(&env),
                    allocation.into_val(&env),
                    unlock_at.into_val(&env),
                ],
            ),
        );

        env.storage()
            .persistent()
            .set(&DataKey::Certificate(cert_id), &cert);
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
        env.storage()
            .instance()
            .get(&DataKey::CertCount)
            .unwrap_or(0)
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

    // ReserveVault reads this to authenticate deposits and reclaims against the
    // operator who actually owns the certificate, rather than one global operator.
    pub fn get_cert_operator(env: Env, cert_id: u64) -> Address {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.operator
    }

    // ReserveVault reads this to lock a certificate's reserve until its expiry.
    pub fn get_cert_expires_at(env: Env, cert_id: u64) -> u64 {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.expires_at
    }

    /// The instant after which nothing can still be proven against this
    /// certificate, and therefore the instant its collateral may unwind.
    ///
    /// Both the ReserveVault (operator's reserve) and AuditorStaking (auditor's
    /// allocation) lock to this, so the two never drift apart.
    pub fn get_cert_settlement_deadline(env: Env, cert_id: u64) -> u64 {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.expires_at.saturating_add(CHALLENGE_WINDOW_SECONDS)
    }

    /// The bound the certificate advertises. The ChallengeManager reads this
    /// for two things: the `BoundExceeded` comparison against the router's
    /// spend counter, and the de-minimis floor of the `ExpiredCertificate`
    /// predicate, which is a percentage of this number rather than a flat
    /// amount.
    pub fn get_cert_bound(env: Env, cert_id: u64) -> i128 {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.bound
    }

    /// The agent named on the certificate.
    ///
    /// The ChallengeManager needs it to answer "has this certificate been
    /// superseded?": `get_cert_id(agent)` is the agent's *current* certificate,
    /// and if that is no longer `cert_id` the operator has published a fresh one
    /// for the same agent. That is what renewal means in this registry — there
    /// is no in-place extension, because `expires_at` is immutable once
    /// published.
    pub fn get_cert_agent(env: Env, cert_id: u64) -> Address {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.agent
    }

    /// Whether the certificate is currently attested and not invalidated.
    /// Deliberately says nothing about expiry: a predicate about post-expiry
    /// activity has to be able to ask this question after `expires_at`.
    pub fn is_cert_verified(env: Env, cert_id: u64) -> bool {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.status == CertStatus::Verified
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
        let cm: Address = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeManager)
            .unwrap();
        cm.require_auth();

        let mut cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");

        cert.status = CertStatus::Invalid;
        env.storage()
            .persistent()
            .set(&DataKey::Certificate(cert_id), &cert);
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
        pub fn allocate(_env: Env, _auditor: Address, _cert_id: u64, _amount: i128, _until: u64) {}
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
        pub fn allocate(_env: Env, _auditor: Address, _cert_id: u64, _amount: i128, _until: u64) {}
    }
}

#[cfg(test)]
// USDC amounts are written as <dollars>_<7 decimals>, e.g. 50_000_0000000 is
// $50,000. Clippy reads that as inconsistent grouping and suggests
// 500_000_000_000, which is the same number with the dollar figure no longer
// legible. The grouping is deliberate.
#[allow(clippy::inconsistent_digit_grouping)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Env,
    };

    /// The slice of stake an auditor puts behind a certificate in these tests.
    const ALLOCATION: i128 = 500_0000000;

    fn setup_with_mock(env: &Env, registered: bool) -> (RegistryClient<'_>, Address) {
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

    fn publish_cert(
        client: &RegistryClient,
        env: &Env,
        operator: &Address,
        agent: &Address,
    ) -> u64 {
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
        client.attest(&auditor, &cert_id, &ALLOCATION);

        let result = client.verify(&agent);
        assert!(result.valid);
        assert_eq!(result.status, CertStatus::Verified);
        assert_eq!(result.auditor_stake, ALLOCATION);
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
        client.attest(&auditor, &cert_id, &ALLOCATION);
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
        client.attest(&auditor, &cert_id, &ALLOCATION);
        client.attest(&auditor, &cert_id, &ALLOCATION); // ikinci kez → panic
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
            &operator,
            &agent,
            &50_000_0000000i128,
            &10_000_0000000i128,
            &2000u64,
            &rv,
            &ast,
        );

        client.attest(&auditor, &cert_id, &ALLOCATION);
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
        client.attest(&auditor, &cert_id, &ALLOCATION);
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
            &operator,
            &agent,
            &50_000_0000000i128,
            &10_000_0000000i128,
            &1000u64,
            &rv,
            &ast,
        );
    }
}
