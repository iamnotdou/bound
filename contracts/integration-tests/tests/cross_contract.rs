//! Offline cross-contract integration harness for Bound Protocol.
//!
//! `docs/PROJECT.md` used to claim that "the full cross-contract slash +
//! compensate path uses live `invoke_contract` calls ... so it is exercised in
//! Layer 2 against real deployed contracts — not in unit tests." That is not
//! true: `soroban_sdk::testutils` can register all five contracts plus a
//! Stellar Asset Contract in one `Env` and drive the whole flow with no
//! network, no credentials and no deployed addresses. This file does that.
//!
//! USDC amounts are written as `<dollars>_<7 decimals>`, e.g. `500_0000000` is
//! $500. Clippy reads that as inconsistent grouping; the grouping is deliberate
//! so the dollar figure stays legible.
#![allow(clippy::inconsistent_digit_grouping)]

use soroban_sdk::{
    testutils::{Address as _, Ledger as _, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal,
};

use auditor_staking::{AuditorStaking, AuditorStakingClient};
use challenge_manager::{
    ChallengeManager, ChallengeManagerClient, ProofType, Verdict as CmVerdict,
};
use fee_escrow::{FeeEscrow, FeeEscrowClient};
use registry::{CertStatus, Registry, RegistryClient};
use reserve_vault::{ReserveVault, ReserveVaultClient};

// ---------------------------------------------------------------------------
// Money
// ---------------------------------------------------------------------------

/// Minimum stake an auditor must hold to count as registered: $500.
const MIN_AUDITOR_STAKE: i128 = 500_0000000;
/// Minimum bond a challenger must post: $100.
const MIN_CHALLENGE_STAKE: i128 = 100_0000000;
/// Starting USDC balance minted to every actor: $10,000. Kept deliberately
/// small — every host operation lands in `test_snapshots/`.
const FUNDING: i128 = 10_000_0000000;

// ---------------------------------------------------------------------------
// The harness
// ---------------------------------------------------------------------------

/// All five Bound contracts plus a test USDC token, wired to each other in a
/// single `Env`, with funded actors.
///
/// Clients are handed out by accessor methods rather than stored, so the struct
/// stays free of self-referential lifetimes.
struct BoundWorld {
    env: Env,

    // contract ids
    token: Address,
    registry: Address,
    vault: Address,
    staking: Address,
    escrow: Address,
    challenge_manager: Address,

    // actors
    token_admin: Address,
    operator: Address,
    agent: Address,
    /// A second, entirely unprivileged operator with their own agent. Used by
    /// the per-certificate isolation tests.
    operator2: Address,
    agent2: Address,
    auditor: Address,
    challenger: Address,
    victim: Address,
    arbiter: Address,
}

impl BoundWorld {
    /// Register every contract, wire them together, and fund the actors.
    ///
    /// The vault holds no unlock timestamp of its own: each certificate's
    /// reserve unlocks at that certificate's own `expires_at`, read from the
    /// Registry when the reserve is funded.
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let token_admin = Address::generate(&env);
        let operator = Address::generate(&env);
        let agent = Address::generate(&env);
        let operator2 = Address::generate(&env);
        let agent2 = Address::generate(&env);
        let auditor = Address::generate(&env);
        let challenger = Address::generate(&env);
        let victim = Address::generate(&env);
        let arbiter = Address::generate(&env);

        // Test USDC. A Stellar Asset Contract registered in-memory needs no
        // network and no trustlines: `mint` credits an address directly.
        let token = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();

        // Register first, initialize second: registry and challenge-manager
        // each need the other's address, so every id must exist up front.
        let registry = env.register(Registry, ());
        let vault = env.register(ReserveVault, ());
        let staking = env.register(AuditorStaking, ());
        let escrow = env.register(FeeEscrow, ());
        let challenge_manager = env.register(ChallengeManager, ());

        RegistryClient::new(&env, &registry).initialize(&challenge_manager, &staking);
        ReserveVaultClient::new(&env, &vault).initialize(&registry, &challenge_manager, &token);
        AuditorStakingClient::new(&env, &staking).initialize(
            &challenge_manager,
            &registry,
            &token,
            &MIN_AUDITOR_STAKE,
        );
        FeeEscrowClient::new(&env, &escrow).initialize(&challenge_manager, &token);
        ChallengeManagerClient::new(&env, &challenge_manager).initialize(
            &registry,
            &staking,
            &vault,
            &escrow,
            &token,
            &arbiter,
            &MIN_CHALLENGE_STAKE,
        );

        let world = Self {
            env,
            token,
            registry,
            vault,
            staking,
            escrow,
            challenge_manager,
            token_admin,
            operator,
            agent,
            operator2,
            agent2,
            auditor,
            challenger,
            victim,
            arbiter,
        };

        for who in [
            world.operator.clone(),
            world.operator2.clone(),
            world.auditor.clone(),
            world.challenger.clone(),
        ] {
            world.mint(&who, FUNDING);
        }

        world
    }

    // --- clients -----------------------------------------------------------

    fn registry(&self) -> RegistryClient<'_> {
        RegistryClient::new(&self.env, &self.registry)
    }
    fn vault(&self) -> ReserveVaultClient<'_> {
        ReserveVaultClient::new(&self.env, &self.vault)
    }
    fn staking(&self) -> AuditorStakingClient<'_> {
        AuditorStakingClient::new(&self.env, &self.staking)
    }
    fn escrow(&self) -> FeeEscrowClient<'_> {
        FeeEscrowClient::new(&self.env, &self.escrow)
    }
    fn cm(&self) -> ChallengeManagerClient<'_> {
        ChallengeManagerClient::new(&self.env, &self.challenge_manager)
    }

    // --- money -------------------------------------------------------------

    fn mint(&self, to: &Address, amount: i128) {
        token::StellarAssetClient::new(&self.env, &self.token).mint(to, &amount);
    }

    fn balance(&self, who: &Address) -> i128 {
        token::Client::new(&self.env, &self.token).balance(who)
    }

    // --- flow steps --------------------------------------------------------

    fn stake_auditor(&self, amount: i128) {
        self.staking().stake(&self.auditor, &amount);
    }

    fn publish_cert(&self, bound: i128, reserve_claim: i128, expires_at: u64) -> u64 {
        self.publish_cert_for(
            &self.operator,
            &self.agent,
            bound,
            reserve_claim,
            expires_at,
        )
    }

    /// Publish on behalf of an arbitrary operator/agent pair, so a single vault
    /// can be made to hold reserves for two unrelated certificates.
    fn publish_cert_for(
        &self,
        operator: &Address,
        agent: &Address,
        bound: i128,
        reserve_claim: i128,
        expires_at: u64,
    ) -> u64 {
        self.registry().publish(
            operator,
            agent,
            &bound,
            &reserve_claim,
            &expires_at,
            &self.vault,
            &self.staking,
        )
    }

    fn attest(&self, cert_id: u64) {
        self.registry().attest(&self.auditor, &cert_id);
    }

    fn deposit_reserve(&self, cert_id: u64, amount: i128) {
        self.vault().deposit(&cert_id, &amount);
    }

    fn reserve_of(&self, cert_id: u64) -> i128 {
        self.vault().get_balance(&cert_id)
    }

    fn deposit_fee(&self, amount: i128) {
        self.escrow()
            .deposit(&self.operator, &self.auditor, &amount);
    }

    fn challenge(&self, cert_id: u64, proof: ProofType, bond: i128) -> u64 {
        self.cm()
            .challenge(&self.challenger, &cert_id, &proof, &self.victim, &bond)
    }

    /// Restore blanket auth mocking after a test has narrowed it with
    /// `mock_auths`.
    fn mock_all_auths(&self) {
        self.env.mock_all_auths();
    }

    fn resolve(&self, challenge_id: u64) {
        self.cm().resolve(&challenge_id);
    }

    // --- time --------------------------------------------------------------

    fn set_time(&self, ts: u64) {
        self.env.ledger().set_timestamp(ts);
    }
}

/// The standard shape used by most tests: auditor staked $600, certificate
/// published with a $5,000 bound claiming a $1,000 reserve, expiring at t=10_000.
const EXPIRES_AT: u64 = 10_000;
const AUDITOR_STAKE: i128 = 600_0000000;
const BOUND: i128 = 5_000_0000000;
const RESERVE_CLAIM: i128 = 1_000_0000000;

fn staked_and_attested() -> (BoundWorld, u64) {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.attest(cert_id);
    (w, cert_id)
}

// ---------------------------------------------------------------------------
// 1. Happy path
// ---------------------------------------------------------------------------

/// Auditor stakes, operator publishes and funds the reserve, auditor attests,
/// `verify` reports the certificate valid — across all five contracts at once.
#[test]
fn happy_path_publishes_funds_attests_and_verifies() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    // Auditor puts up capital and thereby becomes registered.
    assert!(!w.staking().is_registered(&w.auditor));
    w.stake_auditor(AUDITOR_STAKE);
    assert!(w.staking().is_registered(&w.auditor));
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.auditor), FUNDING - AUDITOR_STAKE);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);

    // Operator publishes; the certificate is Pending, so not yet valid.
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Pending
    );
    assert!(!w.registry().verify(&w.agent).valid);

    // Operator pre-funds the reserve with real tokens.
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    assert_eq!(w.vault().get_balance(&cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.operator), FUNDING - RESERVE_CLAIM);

    // Auditor attests. Registry reads staking cross-contract and locks the stake.
    w.attest(cert_id);

    let result = w.registry().verify(&w.agent);
    assert!(result.valid);
    assert_eq!(result.status, CertStatus::Verified);
    assert_eq!(result.bound, BOUND);
    assert_eq!(result.reserve, RESERVE_CLAIM);
    assert_eq!(result.auditor_stake, AUDITOR_STAKE);
    assert_eq!(result.auditor, Some(w.auditor.clone()));
    assert_eq!(result.expires_at, EXPIRES_AT);

    // The attestation bonded the auditor's capital until the cert expires.
    assert_eq!(w.staking().locked_until(&w.auditor), EXPIRES_AT);
    assert!(w.staking().try_release(&w.auditor).is_err());
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
}

/// Once the certificate has expired the bond unwinds cleanly: the auditor can
/// pull their stake back and the operator can reclaim the reserve. Nobody is
/// out of pocket when nothing went wrong.
#[test]
fn clean_expiry_returns_stake_and_reserve() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);

    w.set_time(EXPIRES_AT + 1);
    w.staking().release(&w.auditor);
    w.vault().release_to_operator(&cert_id);

    assert_eq!(w.balance(&w.auditor), FUNDING);
    assert_eq!(w.balance(&w.operator), FUNDING);
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert_eq!(w.vault().get_balance(&cert_id), 0);
}

// ---------------------------------------------------------------------------
// 2. InsufficientReserve — fraud branch
// ---------------------------------------------------------------------------

/// A certificate claiming a $1,000 reserve while the vault only holds $400.
/// The proof verifies on-chain, and settlement moves real money.
///
/// The arithmetic `settle_fraud` commits to, with a live stake S = $600 and a
/// remaining reserve R = $400:
///   reward       = S / 5   = $120  → challenger (20% finder's fee)
///   victim_share = S - S/5 = $480  → victim
///   reserve      = R       = $400  → victim (drained)
///   bond                          → returned to the challenger in full
#[test]
fn insufficient_reserve_fraud_slashes_auditor_and_pays_victim() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);

    // The lie: the certificate claims $1,000 of reserve, only $400 is deposited.
    let actually_deposited = 400_0000000i128;
    w.deposit_reserve(cert_id, actually_deposited);
    w.attest(cert_id);
    assert!(w.registry().verify(&w.agent).valid);

    let bond = MIN_CHALLENGE_STAKE;
    let challenger_before = w.balance(&w.challenger);
    let victim_before = w.balance(&w.victim);
    assert_eq!(victim_before, 0);

    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, bond);

    // The bond is genuinely at risk: it has left the challenger's account.
    assert_eq!(w.balance(&w.challenger), challenger_before - bond);
    assert_eq!(w.balance(&w.challenge_manager), bond);

    w.resolve(challenge_id);

    // --- exact balance movements ---
    let reward = AUDITOR_STAKE / 5; // 120_0000000
    let victim_share = AUDITOR_STAKE - reward; // 480_0000000
    assert_eq!(reward, 120_0000000);
    assert_eq!(victim_share, 480_0000000);

    // Auditor's stake is wiped out — the whole live stake was slashed.
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert_eq!(w.balance(&w.staking), 0);
    // The auditor never gets the slashed capital back.
    assert_eq!(w.balance(&w.auditor), FUNDING - AUDITOR_STAKE);

    // Victim is compensated from the slashed stake *and* the drained reserve.
    assert_eq!(w.balance(&w.victim), victim_share + actually_deposited);
    assert_eq!(w.balance(&w.victim), 880_0000000);

    // Vault is empty and reports it.
    assert_eq!(w.vault().get_balance(&cert_id), 0);
    assert_eq!(w.balance(&w.vault), 0);

    // Challenger: bond returned in full, plus the 20% finder's fee.
    assert_eq!(w.balance(&w.challenger), challenger_before + reward);
    assert_eq!(w.balance(&w.challenge_manager), 0);

    // Certificate is dead and the verdict recorded.
    let ch = w.cm().get_challenge(&challenge_id);
    assert!(ch.verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
    assert!(!w.registry().verify(&w.agent).valid);

    // Conservation: nothing was minted or burned by settlement.
    let total = w.balance(&w.operator)
        + w.balance(&w.auditor)
        + w.balance(&w.challenger)
        + w.balance(&w.victim)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 3);
}

/// The most extreme fraud: a certificate claiming a reserve that was never
/// funded at all. The victim is then paid only out of the auditor's stake.
#[test]
fn insufficient_reserve_fraud_with_empty_vault_pays_from_stake_only() {
    let (w, cert_id) = staked_and_attested();
    assert_eq!(w.vault().get_balance(&cert_id), 0);

    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);

    assert_eq!(w.balance(&w.victim), AUDITOR_STAKE - AUDITOR_STAKE / 5);
    assert_eq!(w.balance(&w.challenger), FUNDING + AUDITOR_STAKE / 5);
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert!(!w.registry().verify(&w.agent).valid);
}

// ---------------------------------------------------------------------------
// 3. InsufficientReserve — no-fraud branch
// ---------------------------------------------------------------------------

/// A fully funded certificate is challenged. `actual < claimed` is false, so
/// the proof does not verify. Nobody is slashed, nobody is paid, and the
/// challenger forfeits their bond to the ChallengeManager.
#[test]
fn insufficient_reserve_no_fraud_forfeits_the_challengers_bond() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM); // fully funded — the claim is honest
    w.attest(cert_id);

    let auditor_before = w.balance(&w.auditor);
    let operator_before = w.balance(&w.operator);
    let bond = MIN_CHALLENGE_STAKE;

    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, bond);
    w.resolve(challenge_id);

    // Verdict recorded as a failure.
    let ch = w.cm().get_challenge(&challenge_id);
    assert!(ch.verdict == CmVerdict::ChallengeFails);

    // Nothing moved, anywhere, except the forfeited bond.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.auditor), auditor_before);
    assert_eq!(w.balance(&w.operator), operator_before);
    assert_eq!(w.vault().get_balance(&cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.victim), 0);

    // The bond stays locked in the ChallengeManager: it is never refunded and,
    // in the current contracts, there is no path that ever pays it out again.
    assert_eq!(w.balance(&w.challenger), FUNDING - bond);
    assert_eq!(w.balance(&w.challenge_manager), bond);

    // The certificate survives the failed challenge intact.
    assert!(w.registry().verify(&w.agent).valid);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
}

/// An over-funded reserve is likewise not fraud: the predicate is
/// `actual < claimed`, not `actual != claimed`.
#[test]
fn over_funded_reserve_is_not_fraud() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM + 1_0000000);
    w.attest(cert_id);

    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);

    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeFails);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.victim), 0);
}

/// A bond below the minimum is rejected before any money moves.
#[test]
fn challenge_below_minimum_bond_is_rejected_without_moving_money() {
    let (w, cert_id) = staked_and_attested();
    let before = w.balance(&w.challenger);

    assert!(w
        .cm()
        .try_challenge(
            &w.challenger,
            &cert_id,
            &ProofType::InsufficientReserve,
            &w.victim,
            &(MIN_CHALLENGE_STAKE - 1),
        )
        .is_err());

    assert_eq!(w.balance(&w.challenger), before);
    assert_eq!(w.balance(&w.challenge_manager), 0);
}

// ---------------------------------------------------------------------------
// 4. Expiry
// ---------------------------------------------------------------------------

/// A verified certificate stops being valid the moment the ledger passes
/// `expires_at` — no transaction required, the check is on read.
#[test]
fn certificate_becomes_invalid_after_expiry() {
    let (w, cert_id) = staked_and_attested();
    assert!(w.registry().verify(&w.agent).valid);

    // Exactly at expiry it is still valid: the check is `timestamp > expires_at`.
    w.set_time(EXPIRES_AT);
    assert!(w.registry().verify(&w.agent).valid);

    w.set_time(EXPIRES_AT + 1);
    let result = w.registry().verify(&w.agent);
    assert!(!result.valid);
    // Status is untouched — expiry is a read-time judgement, not a state change.
    assert_eq!(result.status, CertStatus::Verified);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
}

// ---------------------------------------------------------------------------
// 5. Authorization
// ---------------------------------------------------------------------------

/// Every privileged entry point rejects when nothing is authorized. These are
/// the calls that move other people's money: only the ChallengeManager may
/// slash, drain the vault, or invalidate a certificate.
#[test]
fn privileged_entry_points_reject_unauthorized_callers() {
    let (w, cert_id) = staked_and_attested();
    w.deposit_reserve(cert_id, RESERVE_CLAIM);

    // Drop every mocked signature: from here on, require_auth actually bites.
    w.env.set_auths(&[]);

    // Only the ChallengeManager may slash an auditor.
    assert!(w
        .staking()
        .try_slash(&w.auditor, &w.challenger, &AUDITOR_STAKE)
        .is_err());
    // Only the ChallengeManager may pay a victim out of the reserve.
    assert!(w
        .vault()
        .try_release_to_victim(&cert_id, &w.challenger, &RESERVE_CLAIM)
        .is_err());
    // Only the ChallengeManager may kill a certificate.
    assert!(w.registry().try_invalidate(&cert_id).is_err());
    // Only the Registry may bond an auditor's stake.
    assert!(w.staking().try_lock(&w.auditor, &99_999u64).is_err());
    // Only the operator may deposit into (or reclaim from) the vault.
    assert!(w.vault().try_deposit(&cert_id, &1_0000000i128).is_err());
    assert!(w.vault().try_release_to_operator(&cert_id).is_err());
    // The challenger must sign their own bond.
    assert!(w
        .cm()
        .try_challenge(
            &w.challenger,
            &cert_id,
            &ProofType::InsufficientReserve,
            &w.victim,
            &MIN_CHALLENGE_STAKE,
        )
        .is_err());

    // Nothing moved.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.vault().get_balance(&cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
}

/// Signing as the *wrong* address is not enough: `attest` needs the auditor's
/// own signature, and an attacker authorizing the same invocation does not
/// satisfy it.
#[test]
fn attest_rejects_a_signature_from_the_wrong_address() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);

    let attacker = Address::generate(&w.env);
    w.env.mock_auths(&[MockAuth {
        address: &attacker,
        invoke: &MockAuthInvoke {
            contract: &w.registry,
            fn_name: "attest",
            args: (w.auditor.clone(), cert_id).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w.registry().try_attest(&w.auditor, &cert_id).is_err());
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Pending
    );
}

/// An auditor below the minimum stake cannot attest at all — the Registry
/// reads `is_registered` from AuditorStaking cross-contract to decide.
#[test]
fn under_staked_auditor_cannot_attest() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(MIN_AUDITOR_STAKE - 1);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);

    assert!(!w.staking().is_registered(&w.auditor));
    assert!(w.registry().try_attest(&w.auditor, &cert_id).is_err());
    assert!(!w.registry().verify(&w.agent).valid);
}

// ---------------------------------------------------------------------------
// Known defects — these tests document CURRENT behaviour, scheduled for v2.
// ---------------------------------------------------------------------------

/// KNOWN DEFECT (v2): `Registry::publish` authenticates only the operator and
/// unconditionally overwrites `AgentCert(agent)`. Any address can therefore
/// publish a certificate "for" an agent that never consented, and the newest
/// publisher wins the `verify(agent)` lookup — including an unattested
/// certificate that silently replaces a live, attested one.
///
/// This test asserts the broken behaviour on purpose. v2 must require the
/// agent's authorization and refuse to overwrite a live mapping.
#[test]
fn defect_publish_overwrites_agent_mapping_without_agent_consent() {
    let (w, first_id) = staked_and_attested();
    assert!(w.registry().verify(&w.agent).valid);
    assert_eq!(w.registry().get_cert_id(&w.agent), first_id);

    // A stranger publishes a certificate naming the same agent. The agent never
    // signs anything; only `attacker.require_auth()` is checked.
    let attacker = Address::generate(&w.env);
    let second_id = w.registry().publish(
        &attacker,
        &w.agent,
        &1i128,
        &1i128,
        &EXPIRES_AT,
        &w.vault,
        &w.staking,
    );
    assert_ne!(first_id, second_id);

    // The agent's public certificate is now the stranger's Pending one, and the
    // real, attested certificate is no longer what `verify` returns.
    assert_eq!(w.registry().get_cert_id(&w.agent), second_id);
    let result = w.registry().verify(&w.agent);
    assert!(!result.valid);
    assert_eq!(result.status, CertStatus::Pending);
    assert_eq!(result.bound, 1);

    // The original certificate is still there, just unreachable by agent lookup.
    assert_eq!(
        w.registry().get_certificate(&first_id).status,
        CertStatus::Verified
    );
}

/// KNOWN DEFECT (v2): `initialize()` on every contract has no `require_auth`.
/// Whoever submits the first transaction after deployment owns the wiring.
/// Deployment is only safe today because deploy+initialize are bundled.
///
/// v2 must take an admin address and `require_auth` it.
#[test]
fn defect_initialize_requires_no_authorization() {
    let env = Env::default();
    let anyone = Address::generate(&env);
    let token = env
        .register_stellar_asset_contract_v2(anyone.clone())
        .address();

    let registry = env.register(Registry, ());
    let staking = env.register(AuditorStaking, ());

    // No `mock_all_auths`, no signature of any kind — and it still succeeds.
    env.set_auths(&[]);
    RegistryClient::new(&env, &registry).initialize(&anyone, &staking);
    AuditorStakingClient::new(&env, &staking).initialize(
        &anyone,
        &registry,
        &token,
        &MIN_AUDITOR_STAKE,
    );

    // Re-initialization is the only guard, and it is first-come-first-served.
    assert!(RegistryClient::new(&env, &registry)
        .try_initialize(&anyone, &staking)
        .is_err());
}

/// KNOWN DEFECT (v2), two halves of one bug:
///
/// 1. `ChallengeManager` stores the FeeEscrow address at `initialize()` and
///    then never calls it. `settle_fraud` does not invoke
///    `slash_to_challenger`, so a proven-fraudulent auditor still keeps the
///    audit fee the operator paid them.
/// 2. `FeeEscrow` is a singleton whose `Released` flag is set once and never
///    reset, so it can pay an auditor exactly once in the contract's lifetime.
///    Every subsequent certificate's fee is stranded.
#[test]
fn defect_fee_escrow_is_never_slashed_and_pays_out_only_once() {
    let (w, cert_id) = staked_and_attested();
    let fee = 25_0000000i128;
    w.deposit_fee(fee);
    assert_eq!(w.escrow().get_amount(), fee);

    // Fraud is proven and settled...
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);

    // ...yet the escrow is untouched. `slash_to_challenger` was never called.
    assert_eq!(w.escrow().get_amount(), fee);
    assert_eq!(w.balance(&w.escrow), fee);

    // Worse: the auditor whose fraud was just proven can still collect the fee.
    w.escrow().release_to_auditor();
    assert!(w.escrow().is_released());
    assert_eq!(w.escrow().get_amount(), 0);

    // And the escrow is now permanently spent. A second certificate's fee goes
    // in but can never come out, because `Released` is never reset.
    w.deposit_fee(fee);
    assert_eq!(w.escrow().get_amount(), fee);
    assert!(w.escrow().try_release_to_auditor().is_err());
    assert_eq!(w.balance(&w.escrow), fee);
}

// ---------------------------------------------------------------------------
// Per-certificate reserve accounting (was defect L5 / DESIGN-V2 §9)
// ---------------------------------------------------------------------------

/// This test used to be `defect_reserve_vault_balance_is_shared_across_certificates`,
/// and it asserted the opposite of what it asserts now.
///
/// Defect L5: the ReserveVault kept a single `Balance` and the
/// `InsufficientReserve` proof called `get_balance()` with **no certificate
/// argument**, so any deposit — by anyone, for any certificate — made an
/// entirely unfunded certificate read as fully backed. A genuine fraud proof
/// resolved `ChallengeFails` and the auditor kept their stake. That defeated
/// the protocol's only trustless proof.
///
/// Reserves are now keyed by certificate id, so an unrelated deposit is
/// invisible to this certificate's proof: the fraud is still proven, the
/// auditor is still slashed, and the unrelated reserve is left where it was.
#[test]
fn unrelated_deposit_does_not_rescue_an_unfunded_certificate() {
    let (w, cert_id) = staked_and_attested();
    assert_eq!(w.reserve_of(cert_id), 0);

    // A second, entirely unrelated certificate is funded to the hilt.
    let other_id = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(other_id, RESERVE_CLAIM);
    assert_eq!(w.reserve_of(other_id), RESERVE_CLAIM);
    // ...and the challenged certificate is still empty. Under L5 these were the
    // same number.
    assert_eq!(w.reserve_of(cert_id), 0);

    let victim_before = w.balance(&w.victim);
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);

    // The fraud proof UPHOLDS, and the auditor is slashed to zero.
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert_eq!(w.balance(&w.staking), 0);
    assert_eq!(
        w.balance(&w.victim),
        victim_before + AUDITOR_STAKE - AUDITOR_STAKE / 5
    );
    assert_eq!(w.balance(&w.challenger), FUNDING + AUDITOR_STAKE / 5);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );

    // The unrelated certificate's reserve was neither spent nor counted.
    assert_eq!(w.reserve_of(other_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);
}

/// Two certificates share one vault contract. Funding one must not back the
/// other, and `get_balance` must answer per certificate.
#[test]
fn funding_one_certificate_does_not_back_another_in_the_same_vault() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);

    let cert_a = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    let cert_b = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    assert_ne!(cert_a, cert_b);

    assert_eq!(w.reserve_of(cert_a), 0);
    assert_eq!(w.reserve_of(cert_b), 0);

    let a_funding = 700_0000000i128;
    w.deposit_reserve(cert_a, a_funding);

    // A's money is A's alone.
    assert_eq!(w.reserve_of(cert_a), a_funding);
    assert_eq!(w.reserve_of(cert_b), 0);
    // The vault contract itself custodies the sum.
    assert_eq!(w.balance(&w.vault), a_funding);
    assert_eq!(w.balance(&w.operator), FUNDING - a_funding);
    assert_eq!(w.balance(&w.operator2), FUNDING);

    let b_funding = 250_0000000i128;
    w.deposit_reserve(cert_b, b_funding);
    assert_eq!(w.reserve_of(cert_a), a_funding);
    assert_eq!(w.reserve_of(cert_b), b_funding);
    assert_eq!(w.balance(&w.vault), a_funding + b_funding);
    assert_eq!(w.balance(&w.operator2), FUNDING - b_funding);

    // Each certificate's reserve unlocks at its own certificate's expiry.
    assert!(w.vault().is_locked(&cert_a));
    assert!(w.vault().is_locked(&cert_b));
    assert_eq!(w.vault().get_unlock_at(&cert_a), EXPIRES_AT);
}

/// Settlement isolation: proving fraud against certificate A drains A's reserve
/// to the victim and leaves B's reserve byte-identical.
#[test]
fn fraud_settlement_against_one_certificate_leaves_the_others_reserve_untouched() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);

    let cert_a = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    let cert_b = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.attest(cert_a);

    // A is under-funded (the fraud); B is fully funded and innocent.
    let a_funding = 400_0000000i128;
    w.deposit_reserve(cert_a, a_funding);
    w.deposit_reserve(cert_b, RESERVE_CLAIM);

    let b_before = w.reserve_of(cert_b);
    assert_eq!(b_before, RESERVE_CLAIM);

    let challenge_id = w.challenge(cert_a, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);

    // A's reserve is drained to the victim, alongside the slashed stake.
    assert_eq!(w.reserve_of(cert_a), 0);
    assert_eq!(
        w.balance(&w.victim),
        AUDITOR_STAKE - AUDITOR_STAKE / 5 + a_funding
    );

    // B's reserve did not move by a single stroop, and neither did operator2.
    assert_eq!(w.reserve_of(cert_b), b_before);
    assert_eq!(w.reserve_of(cert_b), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.operator2), FUNDING - RESERVE_CLAIM);

    // B is still live and still verifiable.
    assert_eq!(
        w.registry().get_certificate(&cert_b).status,
        CertStatus::Pending
    );

    // Conservation across every account that holds money in this scenario.
    let total = w.balance(&w.operator)
        + w.balance(&w.operator2)
        + w.balance(&w.auditor)
        + w.balance(&w.challenger)
        + w.balance(&w.victim)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 4);

    // B's operator can still reclaim it in full once B expires.
    w.set_time(EXPIRES_AT + 1);
    w.vault().release_to_operator(&cert_b);
    assert_eq!(w.balance(&w.operator2), FUNDING);
    assert_eq!(w.reserve_of(cert_b), 0);
    assert_eq!(w.balance(&w.vault), 0);
}

/// The second face of defect L5: the vault authenticated deposits against one
/// global `Operator` stored at `initialize()`, so no other operator could fund
/// a reserve at all. Authentication is now per certificate — read live from the
/// Registry — so any operator can fund their own certificate, and only their
/// own.
#[test]
fn an_arbitrary_operator_can_fund_only_their_own_certificate() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    // `operator` is the address the vault would have been pinned to under v1.
    let cert_a = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    // `operator2` holds no privileged position anywhere in the system.
    let cert_b = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);

    // Signing only as operator2, only for cert_b: the deposit succeeds.
    w.env.mock_auths(&[MockAuth {
        address: &w.operator2,
        invoke: &MockAuthInvoke {
            contract: &w.vault,
            fn_name: "deposit",
            args: (cert_b, RESERVE_CLAIM).into_val(&w.env),
            sub_invokes: &[MockAuthInvoke {
                contract: &w.token,
                fn_name: "transfer",
                args: (w.operator2.clone(), w.vault.clone(), RESERVE_CLAIM).into_val(&w.env),
                sub_invokes: &[],
            }],
        },
    }]);
    w.vault().deposit(&cert_b, &RESERVE_CLAIM);
    assert_eq!(w.reserve_of(cert_b), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.operator2), FUNDING - RESERVE_CLAIM);

    // The same signature aimed at somebody else's certificate is refused: the
    // vault requires cert_a's own operator, not whoever is paying.
    w.env.mock_auths(&[MockAuth {
        address: &w.operator2,
        invoke: &MockAuthInvoke {
            contract: &w.vault,
            fn_name: "deposit",
            args: (cert_a, RESERVE_CLAIM).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.vault().try_deposit(&cert_a, &RESERVE_CLAIM).is_err());

    // Nothing moved on the failed attempt.
    assert_eq!(w.reserve_of(cert_a), 0);
    assert_eq!(w.reserve_of(cert_b), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.operator2), FUNDING - RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);

    // Nor may a stranger reclaim another operator's reserve after expiry.
    w.mock_all_auths();
    w.set_time(EXPIRES_AT + 1);
    w.env.mock_auths(&[MockAuth {
        address: &w.operator,
        invoke: &MockAuthInvoke {
            contract: &w.vault,
            fn_name: "release_to_operator",
            args: (cert_b,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.vault().try_release_to_operator(&cert_b).is_err());
    assert_eq!(w.reserve_of(cert_b), RESERVE_CLAIM);
}

// ---------------------------------------------------------------------------
// Flows that cannot be driven offline
// ---------------------------------------------------------------------------

/// The arbiter path is fully drivable offline (see below), but the *subjective*
/// proof types it exists for — BoundExceeded, FakeSignature — have no on-chain
/// evidence in these contracts at all: `resolve` panics with `needs_arbiter`
/// and `resolve_by_arbiter` takes the verdict as a boolean parameter. There is
/// nothing left to test beyond the plumbing, which this covers.
#[test]
fn arbiter_resolves_subjective_proof_types_both_ways() {
    // Fraud found by the arbiter → the same settlement as the trustless path.
    let (w, cert_id) = staked_and_attested();
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    // The trustless resolver refuses: this proof type is not on-chain provable.
    assert!(w.cm().try_resolve(&id).is_err());
    w.cm().resolve_by_arbiter(&id, &true);
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert_eq!(w.balance(&w.victim), AUDITOR_STAKE - AUDITOR_STAKE / 5);

    // No fraud found → the challenger forfeits the bond, nothing else moves.
    let (w2, cert_id2) = staked_and_attested();
    let id2 = w2.challenge(cert_id2, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w2.cm().resolve_by_arbiter(&id2, &false);
    assert!(w2.cm().get_challenge(&id2).verdict == CmVerdict::ChallengeFails);
    assert_eq!(w2.staking().get_stake(&w2.auditor), AUDITOR_STAKE);
    assert_eq!(w2.balance(&w2.victim), 0);
    assert_eq!(w2.balance(&w2.challenger), FUNDING - MIN_CHALLENGE_STAKE);
}

/// Only the named arbiter may rule on a subjective challenge.
#[test]
fn arbiter_resolution_rejects_non_arbiter() {
    let (w, cert_id) = staked_and_attested();
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);

    w.env.set_auths(&[]);
    assert!(w.cm().try_resolve_by_arbiter(&id, &true).is_err());
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::Pending);
    // Sanity: the arbiter is who we wired in.
    assert!(w.arbiter != w.challenger);
    assert!(w.token_admin != w.challenger);
}

/// A resolved challenge cannot be resolved twice — the settlement is not replayable.
#[test]
fn double_resolve_is_rejected_after_a_real_settlement() {
    let (w, cert_id) = staked_and_attested();
    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(id);
    let victim_after_first = w.balance(&w.victim);

    assert!(w.cm().try_resolve(&id).is_err());
    assert!(w.cm().try_resolve_by_arbiter(&id, &true).is_err());
    assert_eq!(w.balance(&w.victim), victim_after_first);
}
