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

// ---------------------------------------------------------------------------
// Storage lifetime — defect L2. This block is the canonical explanation; the
// other contracts carry the same two constants and point back here.
//
// Soroban archives any instance or persistent entry whose TTL lapses, and
// **reaching an archived entry aborts the transaction** rather than returning a
// default. Nothing in this workspace extended any TTL, so two things were
// waiting to happen: a certificate nobody touched for long enough would stop
// being readable, and each contract *instance* is on the same clock, which
// would take the whole contract offline rather than merely one entry.
//
// At the network's ~5s ledger close one day is 17,280 ledgers.
//
//   TTL_EXTEND_TO = 120 days. Chosen against the lifetime of the thing being
//   protected, not picked round: a certificate's own `expires_at` plus
//   `CHALLENGE_WINDOW_SECONDS` (7 days) is the full period during which its
//   entries must stay reachable, and 120 days covers a quarter-long
//   certificate and its window with room to spare. Every entry a certificate
//   creates therefore outlives the certificate.
//
//   TTL_THRESHOLD = 60 days, i.e. half the runway. `extend_ttl` is a no-op
//   when the remaining TTL is already above the threshold, so a threshold at
//   half means at most one rent payment per 60 days of activity instead of one
//   on every single call.
//
// Both sit under the 3,110,400-ledger `max_entry_ttl` the live networks
// configure (the test host allows 6,312,000), so an extension is never
// clamped.
//
// WHO PAYS: TTL extension is rent, charged to the submitter of the transaction
// that triggers the bump. Every bump below sits on a write path, so the payer
// is the operator, agent, auditor, arbiter or challenger who was already
// paying for that call. The protocol never pays, and no read-only path is
// turned into a state change — which is the deliberate residual: a certificate
// that nobody transacts against at all for 120 days still archives. Publishing,
// attesting, depositing, paying a premium or filing a challenge all reset it.
const LEDGERS_PER_DAY: u32 = 17_280;
pub const TTL_THRESHOLD: u32 = 60 * LEDGERS_PER_DAY;
pub const TTL_EXTEND_TO: u32 = 120 * LEDGERS_PER_DAY;

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
    /// DESIGN-V2 §1. Ledger time until which an open **claim window** freezes
    /// this certificate. Written only by the ChallengeManager, through
    /// `set_claim_freeze`, and folded into `get_cert_settlement_deadline` so
    /// that the freeze reuses the one locking mechanism the ReserveVault and
    /// AuditorStaking already read, rather than inventing a second one.
    ClaimFreeze(u64),
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
        Self::bump_instance(&env);
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
        // Defect L1. `operator.require_auth()` alone let *any* funded account
        // publish a certificate naming an agent it does not control, and the
        // unconditional write to `AgentCert(agent)` below then made
        // `verify(agent)` resolve to that junk `Pending` certificate instead of
        // the real one. One transaction, repeatable, and it knocked any agent
        // offline. Requiring the agent's signature too makes bonding consent
        // mutual, and matches PaymentRouter::enroll, which has always required
        // both parties for exactly this reason.
        //
        // WHAT THIS COSTS: publishing now needs two signatures — the operator's
        // and the agent's. Soroban allows one contract call per transaction, so
        // both must be collected into the *same* transaction's auth entries; a
        // UI cannot split this into two sequential submissions. Any client that
        // signs as the operator and submits must first obtain the agent's
        // signed authorization entry.
        operator.require_auth();
        agent.require_auth();

        if bound <= 0 {
            panic!("invalid_bound");
        }
        if reserve_amount <= 0 {
            panic!("invalid_reserve");
        }
        if expires_at <= env.ledger().timestamp() {
            panic!("expiry_in_past");
        }

        // Defect L1, second half: should an existing mapping be overwritable at
        // all? Yes — but not while it is under dispute.
        //
        // Overwriting is how renewal works in this registry: `expires_at` is
        // immutable once published, so an operator renews by publishing a fresh
        // certificate for the same agent (see `get_cert_agent`). Forbidding the
        // overwrite outright would remove renewal, and with `agent.require_auth()`
        // above a third party can no longer perform one anyway.
        //
        // The one case that must still be refused is republishing over a
        // certificate whose claim window is open. The old certificate's
        // collateral stays locked by cert id regardless, so no money escapes —
        // but `verify(agent)` would start resolving to a clean `Pending`
        // certificate and a counterparty would see no sign of the live breach.
        // The operator and the agent can renew the moment the window closes.
        if let Some(prev_id) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::AgentCert(agent.clone()))
        {
            if env.ledger().timestamp() < Self::claim_freeze(&env, prev_id) {
                panic!("claim_window_open");
            }
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
            .set(&DataKey::AgentCert(agent.clone()), &cert_id);
        env.storage().instance().set(&DataKey::CertCount, &cert_id);

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Certificate(cert_id));
        Self::bump(&env, &DataKey::AgentCert(agent));

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

        // DESIGN-V2 §1. A frozen certificate takes no new attestation. Without
        // this an operator whose certificate is under a live claim window could
        // walk a fresh auditor onto it and put new collateral at risk for a
        // breach that has already been filed.
        if env.ledger().timestamp() < Self::claim_freeze(&env, cert_id) {
            panic!("claim_window_open");
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

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Certificate(cert_id));
        Self::bump(&env, &DataKey::AgentCert(cert.agent));
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

    /// When the certificate was published.
    ///
    /// The PremiumVault reads this together with `get_cert_expires_at` to price
    /// coverage over `expires_at - issued_at`. Both fields are immutable once
    /// published, which is the point: a premium priced from `now` would be a
    /// function of when the operator chose to pay it, and an operator would
    /// simply wait until the instant before expiry and buy a year of coverage
    /// for a day's price.
    pub fn get_cert_issued_at(env: Env, cert_id: u64) -> u64 {
        let cert: Certificate = env
            .storage()
            .persistent()
            .get(&DataKey::Certificate(cert_id))
            .expect("certificate_not_found");
        cert.issued_at
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
        let base = cert.expires_at.saturating_add(CHALLENGE_WINDOW_SECONDS);
        // DESIGN-V2 §1: an open claim window extends the deadline. Expiry must
        // not let the collateral escape from underneath a window that is still
        // aggregating claims, so the later of the two always wins.
        let freeze = Self::claim_freeze(&env, cert_id);
        if freeze > base {
            freeze
        } else {
            base
        }
    }

    /// DESIGN-V2 §1. Freeze this certificate until `until`, or lift the freeze
    /// by passing `0`.
    ///
    /// ChallengeManager-only, exactly like `invalidate`: the claim window is
    /// the ChallengeManager's concept and nobody else may open or close one.
    /// The freeze is expressed as a settlement deadline rather than a separate
    /// flag so that both money contracts get it for free — they already refuse
    /// to release before `get_cert_settlement_deadline`.
    pub fn set_claim_freeze(env: Env, cert_id: u64, until: u64) {
        let cm: Address = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeManager)
            .unwrap();
        cm.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::ClaimFreeze(cert_id), &until);

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::ClaimFreeze(cert_id));
        // The window outlives the freeze flag if the certificate itself lapses,
        // so keep the certificate reachable for as long as the freeze is.
        Self::bump(&env, &DataKey::Certificate(cert_id));
    }

    /// Ledger time until which an open claim window holds this certificate
    /// frozen. `0` means no window is open.
    pub fn get_claim_freeze(env: Env, cert_id: u64) -> u64 {
        Self::claim_freeze(&env, cert_id)
    }

    /// Whether a claim window is open on this certificate right now.
    pub fn is_frozen(env: Env, cert_id: u64) -> bool {
        env.ledger().timestamp() < Self::claim_freeze(&env, cert_id)
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

    fn claim_freeze(env: &Env, cert_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::ClaimFreeze(cert_id))
            .unwrap_or(0)
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

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Certificate(cert_id));
        Self::bump(&env, &DataKey::AgentCert(cert.agent));
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
        testutils::{
            storage::{Instance as _, Persistent as _},
            Address as _, Ledger as _, MockAuth, MockAuthInvoke,
        },
        Env, IntoVal,
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

    // ---- Defect L1: publishing requires the agent's consent ----------------

    const TEST_BOUND: i128 = 50_000_0000000;
    const TEST_RESERVE: i128 = 10_000_0000000;
    const TEST_EXPIRY: u64 = 9_999_999;

    /// The exact argument tuple `publish` is called with in the L1 tests.
    /// `MockAuth` matches on the arguments, so this has to mirror the call.
    fn publish_args(
        env: &Env,
        operator: &Address,
        agent: &Address,
        rv: &Address,
        ast: &Address,
    ) -> soroban_sdk::Vec<soroban_sdk::Val> {
        (
            operator.clone(),
            agent.clone(),
            TEST_BOUND,
            TEST_RESERVE,
            TEST_EXPIRY,
            rv.clone(),
            ast.clone(),
        )
            .into_val(env)
    }

    #[test]
    fn test_publish_without_agent_auth_is_rejected() {
        let env = Env::default();
        let (client, _) = setup_with_mock(&env, true);
        env.ledger().set_timestamp(1000);

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let rv = Address::generate(&env);
        let ast = Address::generate(&env);
        let args = publish_args(&env, &operator, &agent, &rv, &ast);

        // Only the operator signs — exactly the pre-fix situation.
        let res = client
            .mock_auths(&[MockAuth {
                address: &operator,
                invoke: &MockAuthInvoke {
                    contract: &client.address,
                    fn_name: "publish",
                    args: args.clone(),
                    sub_invokes: &[],
                },
            }])
            .try_publish(
                &operator,
                &agent,
                &TEST_BOUND,
                &TEST_RESERVE,
                &TEST_EXPIRY,
                &rv,
                &ast,
            );

        assert!(res.is_err(), "publish must fail without the agent's auth");
        // And nothing was written: the agent still has no certificate.
        assert!(!client.verify(&agent).valid);
        assert_eq!(client.get_cert_count(), 0);
    }

    #[test]
    fn test_publish_with_both_signatures_succeeds() {
        let env = Env::default();
        let (client, _) = setup_with_mock(&env, true);
        env.ledger().set_timestamp(1000);

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let rv = Address::generate(&env);
        let ast = Address::generate(&env);
        let args = publish_args(&env, &operator, &agent, &rv, &ast);

        let cert_id = client
            .mock_auths(&[
                MockAuth {
                    address: &operator,
                    invoke: &MockAuthInvoke {
                        contract: &client.address,
                        fn_name: "publish",
                        args: args.clone(),
                        sub_invokes: &[],
                    },
                },
                MockAuth {
                    address: &agent,
                    invoke: &MockAuthInvoke {
                        contract: &client.address,
                        fn_name: "publish",
                        args: args.clone(),
                        sub_invokes: &[],
                    },
                },
            ])
            .publish(
                &operator,
                &agent,
                &TEST_BOUND,
                &TEST_RESERVE,
                &TEST_EXPIRY,
                &rv,
                &ast,
            );

        assert_eq!(cert_id, 1);
        assert_eq!(client.get_cert_id(&agent), 1);
    }

    #[test]
    fn test_third_party_cannot_hijack_an_existing_mapping() {
        let env = Env::default();
        let (client, _) = setup_with_mock(&env, true);
        env.ledger().set_timestamp(1000);

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);

        // The real, attested certificate.
        env.mock_all_auths();
        let real_id = publish_cert(&client, &env, &operator, &agent);
        client.attest(&auditor, &real_id, &ALLOCATION);
        assert!(client.verify(&agent).valid);

        // An unrelated funded account tries to overwrite the mapping, signing
        // only for itself. This is the whole L1 attack.
        let attacker = Address::generate(&env);
        let rv = Address::generate(&env);
        let ast = Address::generate(&env);
        let args = publish_args(&env, &attacker, &agent, &rv, &ast);

        let res = client
            .mock_auths(&[MockAuth {
                address: &attacker,
                invoke: &MockAuthInvoke {
                    contract: &client.address,
                    fn_name: "publish",
                    args: args.clone(),
                    sub_invokes: &[],
                },
            }])
            .try_publish(
                &attacker,
                &agent,
                &TEST_BOUND,
                &TEST_RESERVE,
                &TEST_EXPIRY,
                &rv,
                &ast,
            );

        assert!(res.is_err(), "a third party must not be able to publish");
        // The agent still resolves to the real, verified certificate.
        assert_eq!(client.get_cert_id(&agent), real_id);
        assert!(client.verify(&agent).valid);
    }

    #[test]
    fn test_renewal_by_the_same_parties_still_works() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);
        env.ledger().set_timestamp(1000);

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);

        let first = publish_cert(&client, &env, &operator, &agent);
        let second = publish_cert(&client, &env, &operator, &agent);

        assert_ne!(first, second);
        assert_eq!(client.get_cert_id(&agent), second);
    }

    #[test]
    #[should_panic(expected = "claim_window_open")]
    fn test_cannot_republish_over_a_frozen_certificate() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, cm) = setup_with_mock(&env, true);
        env.ledger().set_timestamp(1000);

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);

        let cert_id = publish_cert(&client, &env, &operator, &agent);
        // The ChallengeManager opens a claim window on it.
        let _ = cm;
        client.set_claim_freeze(&cert_id, &5000u64);

        // Renewal would otherwise wash the live dispute out of `verify`.
        publish_cert(&client, &env, &operator, &agent);
    }

    // ---- Defect L2: entries survive long enough to be archived -------------

    /// A contract that writes a persistent entry and never extends anything —
    /// the state every contract in this workspace was in before defect L2 was
    /// fixed. It exists so the test below proves archival is real in this host
    /// rather than assuming it.
    #[contract]
    pub struct Unbumped;

    #[contractimpl]
    impl Unbumped {
        pub fn put(env: Env) {
            env.storage()
                .persistent()
                .set(&Symbol::new(&env, "k"), &1i128);
        }
        pub fn get(env: Env) -> i128 {
            env.storage()
                .persistent()
                .get(&Symbol::new(&env, "k"))
                .unwrap_or(-1)
        }
    }

    /// The control. Without a TTL extension the test host's default persistent
    /// minimum is 4,096 ledgers, and once that lapses the entry is archived and
    /// the *call* fails — it does not read back as a default. This is the
    /// failure mode L2 was about, demonstrated rather than asserted in prose.
    #[test]
    #[should_panic(expected = "Storage, InternalError")]
    fn test_an_unbumped_entry_really_does_archive() {
        let env = Env::default();
        let id = env.register(Unbumped, ());
        let client = UnbumpedClient::new(&env, &id);
        client.put();

        env.as_contract(&id, || {
            assert!(
                env.storage().persistent().get_ttl(&Symbol::new(&env, "k")) < 4_096,
                "the host's default persistent TTL should be the 4,096-ledger minimum"
            );
        });

        // Past the TTL. The host aborts the whole call — note it names the
        // *instance* key, so an un-bumped contract does not merely lose one
        // entry, it stops answering at all. This escalates to a host error
        // rather than a contract error, so `try_get` cannot catch it either;
        // hence `should_panic` rather than an `is_err` assertion.
        env.ledger().set_sequence_number(100_000);
        client.get();
    }

    /// The fix. Same clock, same host, but the Registry extends its TTLs on the
    /// write path, so everything the certificate needs is still live.
    #[test]
    fn test_published_certificate_survives_the_archival_horizon() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = setup_with_mock(&env, true);
        env.ledger().set_timestamp(1000);

        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let auditor = Address::generate(&env);
        let cert_id = publish_cert(&client, &env, &operator, &agent);
        client.attest(&auditor, &cert_id, &ALLOCATION);

        // The TTLs were actually extended, not merely written. `extend_ttl`
        // reports the remaining lifetime, which is `extend_to` counted from the
        // current ledger.
        env.as_contract(&client.address, || {
            assert!(
                env.storage().instance().get_ttl() >= TTL_EXTEND_TO - 1,
                "instance TTL was not extended"
            );
            assert!(
                env.storage()
                    .persistent()
                    .get_ttl(&DataKey::Certificate(cert_id))
                    >= TTL_EXTEND_TO - 1,
                "certificate TTL was not extended"
            );
            assert!(
                env.storage()
                    .persistent()
                    .get_ttl(&DataKey::AgentCert(agent.clone()))
                    >= TTL_EXTEND_TO - 1,
                "agent mapping TTL was not extended"
            );
        });

        // Past the horizon that archived the control contract's entry, and then
        // some. Both the instance and the two persistent entries are still
        // reachable, so `verify` still answers.
        env.ledger().set_sequence_number(100_000);
        assert_eq!(client.get_cert_id(&agent), cert_id);
        assert!(client.verify(&agent).valid);
        assert_eq!(
            client.get_certificate(&cert_id).status,
            CertStatus::Verified
        );
    }
}
