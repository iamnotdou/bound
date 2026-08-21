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
    symbol_short,
    testutils::{Address as _, Events as _, Ledger as _, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal, Symbol, TryFromVal,
};

use auditor_staking::{AuditorStaking, AuditorStakingClient};
use challenge_manager::{
    ChallengeManager, ChallengeManagerClient, ProofType, Verdict as CmVerdict,
};
use fee_escrow::{FeeEscrow, FeeEscrowClient};
use payment_router::{PaymentRouter, PaymentRouterClient};
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
/// The challenge window the Registry publishes: 7 days. The reserve and the
/// auditor's allocation both stay locked until `expires_at + CHALLENGE_WINDOW`.
const CHALLENGE_WINDOW: u64 = 7 * 24 * 60 * 60;
/// The challenger's fee, in basis points of proven harm (10%).
const CHALLENGER_FEE_BPS: i128 = 1_000;
/// The flat hygiene bounty the ChallengeManager pays when a proof is true but
/// no harm is evidenced.
const HYGIENE_BOUNTY: i128 = 10_0000000;

// ---------------------------------------------------------------------------
// The harness
// ---------------------------------------------------------------------------

/// All Bound contracts plus a test USDC token, wired to each other in a
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
    router: Address,

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
    /// Where every slashed stroop goes, and the only place it may go.
    treasury: Address,
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
        let treasury = Address::generate(&env);

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
        let router = env.register(PaymentRouter, ());

        RegistryClient::new(&env, &registry).initialize(&challenge_manager, &staking);
        ReserveVaultClient::new(&env, &vault).initialize(&registry, &challenge_manager, &token);
        AuditorStakingClient::new(&env, &staking).initialize(
            &challenge_manager,
            &registry,
            &token,
            &MIN_AUDITOR_STAKE,
        );
        FeeEscrowClient::new(&env, &escrow).initialize(&challenge_manager, &token);
        PaymentRouterClient::new(&env, &router).initialize(&registry, &token);
        ChallengeManagerClient::new(&env, &challenge_manager).initialize(
            &registry,
            &staking,
            &vault,
            &escrow,
            &token,
            &arbiter,
            &treasury,
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
            router,
            token_admin,
            operator,
            agent,
            operator2,
            agent2,
            auditor,
            challenger,
            victim,
            arbiter,
            treasury,
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
    fn router(&self) -> PaymentRouterClient<'_> {
        PaymentRouterClient::new(&self.env, &self.router)
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

    /// Attest, allocating the whole standard stake to this certificate.
    fn attest(&self, cert_id: u64) {
        self.attest_with(cert_id, ALLOCATION);
    }

    /// Attest, naming exactly how much of the auditor's free stake stands
    /// behind this certificate.
    fn attest_with(&self, cert_id: u64, allocation: i128) {
        self.registry().attest(&self.auditor, &cert_id, &allocation);
    }

    fn allocation_of(&self, cert_id: u64) -> i128 {
        self.staking().get_allocation(&cert_id)
    }

    fn free_stake(&self) -> i128 {
        self.staking().get_free_stake(&self.auditor)
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
/// How much of that stake the standard attestation allocates to the certificate:
/// all of it, so most tests read as they did before per-certificate allocation.
const ALLOCATION: i128 = AUDITOR_STAKE;
const BOUND: i128 = 5_000_0000000;
const RESERVE_CLAIM: i128 = 1_000_0000000;
/// The instant the standard certificate's collateral may finally unwind.
const SETTLEMENT_DEADLINE: u64 = EXPIRES_AT + CHALLENGE_WINDOW;

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

    // The attestation allocated the auditor's capital to this certificate, and
    // locked it until the challenge window closes — not until expiry.
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(
        w.staking().allocation_unlock_at(&cert_id),
        SETTLEMENT_DEADLINE
    );
    assert_eq!(w.staking().locked_until(&w.auditor), SETTLEMENT_DEADLINE);
    assert_eq!(w.free_stake(), 0);

    // `release` withdraws free stake only, so it moves nothing at all here.
    // Allocated capital is locked because a live certificate stands on it.
    w.staking().release(&w.auditor);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    // And the allocation cannot be freed early either.
    assert!(w.staking().try_release_allocation(&cert_id).is_err());
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

    // Expiry alone is not enough: the challenge window has to close first.
    w.set_time(EXPIRES_AT + 1);
    assert!(w.staking().try_release_allocation(&cert_id).is_err());
    assert!(w.vault().try_release_to_operator(&cert_id).is_err());

    w.set_time(SETTLEMENT_DEADLINE);
    w.staking().release_allocation(&cert_id);
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

/// The waterfall, with a reserve big enough to cover the harm in full.
///
/// Claim $1,000, deposit $900 → proven harm is the $100 shortfall.
///   victim      = min(harm, reserve)        = $100  <- the operator's reserve
///   fee         = 10% of proven harm        = $10   <- the same reserve
///   slash       = harm not covered, capped  = $0    <- nothing left to slash
///   allocation retires in full: $600 back to the auditor's free stake
#[test]
fn insufficient_reserve_fraud_pays_victim_and_fee_from_the_operators_own_reserve() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);

    let deposited = 900_0000000i128;
    w.deposit_reserve(cert_id, deposited);
    w.attest(cert_id);

    let harm = RESERVE_CLAIM - deposited;
    assert_eq!(harm, 100_0000000);
    let fee = harm * CHALLENGER_FEE_BPS / 10_000;
    assert_eq!(fee, 10_0000000);

    let bond = MIN_CHALLENGE_STAKE;
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, bond);
    w.resolve(challenge_id);

    // 1. The victim is compensated out of the operator's own reserve.
    assert_eq!(w.balance(&w.victim), harm);
    // 2. The challenger's fee is a share of proven harm, from the same reserve.
    assert_eq!(w.balance(&w.challenger), FUNDING + fee);
    // 3. Nothing was slashed: the operator's reserve covered the whole harm.
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    // 5. The allocation retired, so every stroop is free stake again.
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    // 6. Certificate dead, bond returned.
    assert_eq!(w.reserve_of(cert_id), deposited - harm - fee);
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
}

/// The most extreme fraud: a certificate claiming a reserve that was never
/// funded at all.
///
/// There is no operator reserve, so **the victim is paid nothing** — victim
/// compensation comes only from the operator's own money, never from the
/// auditor's. The auditor is still slashed, capped by their allocation, and
/// every slashed stroop lands in the treasury.
///
/// This is the test that proves the prize is gone: the party who filed the
/// proof and the party named as victim between them receive none of the
/// auditor's $600.
#[test]
fn insufficient_reserve_slash_goes_to_the_treasury_and_never_to_victim_or_challenger() {
    let (w, cert_id) = staked_and_attested();
    assert_eq!(w.reserve_of(cert_id), 0);

    let bond = MIN_CHALLENGE_STAKE;
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, bond);
    w.resolve(challenge_id);

    // harm = $1,000 claimed - $0 actual; payable is capped by the collateral,
    // which here is the $600 allocation alone.
    let slash = ALLOCATION;

    // The treasury received the slash, in full and to the stroop.
    assert_eq!(w.balance(&w.treasury), slash);

    // The victim received no part of it. They received nothing at all, because
    // the operator posted nothing to compensate them with.
    assert_eq!(w.balance(&w.victim), 0);
    // The challenger got their bond back and not one stroop more: the fee is a
    // share of harm paid from the reserve, and the reserve is empty.
    assert_eq!(w.balance(&w.challenger), FUNDING);

    // The auditor's allocation is spent and the certificate is dead.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE - slash);
    assert_eq!(w.free_stake(), 0);
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE - slash);
    assert!(!w.registry().verify(&w.agent).valid);
}

/// The mixed case, and the clearest statement of the ordering: the operator's
/// reserve is drawn first, and only the harm it could not cover reaches the
/// auditor's allocation.
///
/// Claim $1,000, deposit $400, allocation $600 → harm $600.
///   victim = min(600, 400) = $400   <- reserve, exhausted
///   fee    = min(60, 0)    = $0     <- nothing left in the reserve
///   slash  = 600 - 400     = $200   <- treasury, capped by the $600 allocation
///   remainder $400 returns to the auditor's free stake
#[test]
fn insufficient_reserve_fraud_draws_the_reserve_first_then_the_allocation() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);

    let deposited = 400_0000000i128;
    w.deposit_reserve(cert_id, deposited);
    w.attest(cert_id);

    let challenger_before = w.balance(&w.challenger);
    let bond = MIN_CHALLENGE_STAKE;
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, bond);

    // The bond is genuinely at risk: it has left the challenger's account.
    assert_eq!(w.balance(&w.challenger), challenger_before - bond);
    assert_eq!(w.balance(&w.challenge_manager), bond);

    w.resolve(challenge_id);

    let harm = RESERVE_CLAIM - deposited; // $600
    let slash = harm - deposited; // $200
    assert_eq!(harm, 600_0000000);
    assert_eq!(slash, 200_0000000);

    assert_eq!(w.balance(&w.victim), deposited);
    assert_eq!(w.balance(&w.treasury), slash);
    assert_eq!(w.balance(&w.challenger), challenger_before); // bond back, fee $0
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.balance(&w.vault), 0);

    // The unslashed remainder is free stake again — not stranded on a dead cert.
    assert_eq!(w.free_stake(), ALLOCATION - slash);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE - slash);
    assert_eq!(w.allocation_of(cert_id), 0);

    let ch = w.cm().get_challenge(&challenge_id);
    assert!(ch.verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );

    // Conservation: nothing was minted or burned by settlement — treasury included.
    let total = w.balance(&w.operator)
        + w.balance(&w.auditor)
        + w.balance(&w.challenger)
        + w.balance(&w.victim)
        + w.balance(&w.treasury)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 3);
}

// ---------------------------------------------------------------------------
// 2b. The headline: self-dealing is a wash
// ---------------------------------------------------------------------------

/// **An operator manufactures a true proof against itself and extracts nothing.**
///
/// The operator under-funds its own certificate on purpose, then files the
/// challenge itself and names a second address it controls as the victim. Every
/// step is legitimate: the proof is real, the challenge is real, the settlement
/// is real. The question this test answers is not "can the proof be forged" —
/// it cannot — but "does manufacturing a true proof pay?"
///
/// The colluding set is {operator, sink}. Its net position must not improve.
#[test]
fn self_dealing_operator_extracts_nothing_from_a_manufactured_proof() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);

    // `sink` is the operator's second address: the "victim" it will name, and
    // the account it will file the challenge from.
    let sink = w.operator2.clone();

    let deposited = 900_0000000i128;
    w.deposit_reserve(cert_id, deposited);
    w.attest(cert_id);

    // --- before ---
    let operator_before = w.balance(&w.operator);
    let sink_before = w.balance(&sink);
    let reserve_before = w.reserve_of(cert_id);
    let colluders_before = operator_before + sink_before + reserve_before;
    assert_eq!(operator_before, FUNDING - deposited);
    assert_eq!(sink_before, FUNDING);
    assert_eq!(reserve_before, deposited);
    assert_eq!(colluders_before, FUNDING * 2);

    let auditor_stake_before = w.staking().get_stake(&w.auditor);
    let treasury_before = w.balance(&w.treasury);
    assert_eq!(auditor_stake_before, AUDITOR_STAKE);
    assert_eq!(treasury_before, 0);

    // The operator's own address files the challenge, naming its own sink as
    // the harmed counterparty. Victim naming is exactly as permissive as it
    // always was — the waterfall, not the predicate, is what neutralises it.
    let bond = MIN_CHALLENGE_STAKE;
    let challenge_id = w.cm().challenge(
        &sink,
        &cert_id,
        &ProofType::InsufficientReserve,
        &sink,
        &bond,
    );
    w.resolve(challenge_id);
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);

    // --- after ---
    let operator_after = w.balance(&w.operator);
    let sink_after = w.balance(&sink);
    let reserve_after = w.reserve_of(cert_id);
    let colluders_after = operator_after + sink_after + reserve_after;

    // THE ASSERTION. The colluding pair's total position is exactly what it was.
    // Every stroop that reached the "victim" and every stroop of "fee" came out
    // of the operator's own reserve: left pocket to right pocket.
    assert_eq!(colluders_after, colluders_before);
    assert_eq!(colluders_after, FUNDING * 2);

    // And, itemised, so a future reader can see nothing was smuggled in:
    let harm = RESERVE_CLAIM - deposited; // $100
    let fee = harm * CHALLENGER_FEE_BPS / 10_000; // $10
    assert_eq!(operator_after, operator_before); // untouched
    assert_eq!(sink_after, sink_before + harm + fee); // paid, from its own reserve
    assert_eq!(reserve_after, reserve_before - harm - fee); // by exactly that much

    // The auditor's capital was not touched at all: the operator's reserve
    // covered the harm it manufactured, so there was nothing left to slash.
    assert_eq!(w.staking().get_stake(&w.auditor), auditor_stake_before);
    assert_eq!(w.balance(&w.staking), auditor_stake_before);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.treasury), treasury_before);

    // Nobody outside the colluding pair gained or lost anything.
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.challenger), FUNDING);
    assert_eq!(w.balance(&w.auditor), FUNDING - AUDITOR_STAKE);
    assert_eq!(w.balance(&w.challenge_manager), 0);

    // All the attack achieved was destroying the operator's own certificate.
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
}

/// The same manufactured proof with **no reserve at all** — the version that
/// paid best under v1, where it handed the colluding pair the auditor's entire
/// live stake minus the finder's fee they also collected.
///
/// Now the pair receives nothing and the auditor's slashed capital goes to the
/// treasury, an address none of them controls.
#[test]
fn self_dealing_against_an_empty_reserve_hands_the_colluders_nothing() {
    let (w, cert_id) = staked_and_attested();
    let sink = w.operator2.clone();

    let colluders_before = w.balance(&w.operator) + w.balance(&sink);
    assert_eq!(colluders_before, FUNDING * 2);

    let id = w.cm().challenge(
        &sink,
        &cert_id,
        &ProofType::InsufficientReserve,
        &sink,
        &MIN_CHALLENGE_STAKE,
    );
    w.resolve(id);

    // Not one stroop better off, and the whole allocation is in the treasury.
    assert_eq!(w.balance(&w.operator) + w.balance(&sink), colluders_before);
    assert_eq!(w.balance(&w.treasury), ALLOCATION);
    assert_eq!(w.balance(&w.victim), 0);
    // Under v1 this line read `sink_before + 480_0000000`.
    assert_eq!(w.balance(&sink), FUNDING);
}

// ---------------------------------------------------------------------------
// 2c. The two caps on the slash
// ---------------------------------------------------------------------------

/// **Capped by allocation.** A huge harm against a huge stake still only draws
/// the slice the auditor put behind *this* certificate — and the rest of the
/// stake, which was never allocated, is untouched free capital afterwards.
#[test]
fn slash_is_capped_by_the_certificates_allocation() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    // A well-capitalised auditor: $9,000 staked, but only $600 behind this cert.
    let big_stake = 9_000_0000000i128;
    w.staking().stake(&w.auditor, &big_stake);

    // A $5,000 claim with nothing deposited: harm is $5,000, far above the
    // allocation.
    let claim = 5_000_0000000i128;
    let cert_id = w.publish_cert(BOUND, claim, EXPIRES_AT);
    w.attest_with(cert_id, ALLOCATION);
    assert_eq!(w.free_stake(), big_stake - ALLOCATION);

    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    // Only the allocation was slashed, not the book.
    assert_eq!(w.balance(&w.treasury), ALLOCATION);
    assert_eq!(w.staking().get_stake(&w.auditor), big_stake - ALLOCATION);
    // Nothing is stranded: the remaining stake is free and usable immediately.
    assert_eq!(w.free_stake(), big_stake - ALLOCATION);
    assert_eq!(w.staking().get_allocated(&w.auditor), 0);
    assert_eq!(w.allocation_of(cert_id), 0);
    assert!(w.staking().is_registered(&w.auditor));
}

/// **Capped by harm.** A $10 shortfall against a $5,000 allocation costs the
/// auditor $10 — and the $4,990 remainder returns to free stake.
///
/// This is the line that stops a manufactured trivial breach from costing an
/// auditor their whole bond.
#[test]
fn slash_is_capped_by_proven_harm() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    let big_stake = 9_000_0000000i128;
    let big_allocation = 5_000_0000000i128;
    w.staking().stake(&w.auditor, &big_stake);

    // Claim $1,000, deposit $990: a $10 harm.
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    let deposited = 990_0000000i128;
    w.deposit_reserve(cert_id, deposited);
    w.attest_with(cert_id, big_allocation);
    assert_eq!(w.allocation_of(cert_id), big_allocation);

    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    let harm = RESERVE_CLAIM - deposited;
    assert_eq!(harm, 10_0000000);

    // The reserve covered the whole $10, so the slash is $0 — bounded by harm
    // long before the $5,000 allocation ever came into play.
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.balance(&w.victim), harm);
    assert_eq!(w.staking().get_stake(&w.auditor), big_stake);
    assert_eq!(w.free_stake(), big_stake);
}

/// The same cap where the reserve is empty, so the slash is non-zero and the
/// binding constraint is unambiguously *harm*: a $10 harm, a $5,000 allocation,
/// a $10 slash.
#[test]
fn slash_is_capped_by_harm_even_with_no_reserve_to_draw_on() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    let big_stake = 9_000_0000000i128;
    let big_allocation = 5_000_0000000i128;
    w.staking().stake(&w.auditor, &big_stake);

    // A certificate claiming only $10 of reserve, and funded with nothing.
    let tiny_claim = 10_0000000i128;
    let cert_id = w.publish_cert(BOUND, tiny_claim, EXPIRES_AT);
    w.attest_with(cert_id, big_allocation);

    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert_eq!(w.balance(&w.treasury), tiny_claim);
    assert_eq!(w.staking().get_stake(&w.auditor), big_stake - tiny_claim);
    assert_eq!(w.free_stake(), big_stake - tiny_claim);
    // Victim and challenger got none of it.
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.challenger), FUNDING);
}

/// **Allocation isolation.** Slashing certificate A must not touch the
/// allocation behind certificate B, held by the same auditor in the same
/// contract.
#[test]
fn slashing_one_certificate_leaves_the_other_certificates_allocation_untouched() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    let big_stake = 3_000_0000000i128;
    w.staking().stake(&w.auditor, &big_stake);

    let alloc_a = 600_0000000i128;
    let alloc_b = 800_0000000i128;

    let cert_a = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    let cert_b = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.attest_with(cert_a, alloc_a);
    w.attest_with(cert_b, alloc_b);

    assert_eq!(w.allocation_of(cert_a), alloc_a);
    assert_eq!(w.allocation_of(cert_b), alloc_b);
    assert_eq!(w.staking().get_allocated(&w.auditor), alloc_a + alloc_b);
    assert_eq!(w.free_stake(), big_stake - alloc_a - alloc_b);

    // A is unfunded: harm $1,000, so A's whole $600 allocation is slashed.
    let id = w.challenge(cert_a, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert_eq!(w.balance(&w.treasury), alloc_a);
    assert_eq!(w.allocation_of(cert_a), 0);

    // B's allocation is unchanged to the stroop, and so is B itself.
    assert_eq!(w.allocation_of(cert_b), alloc_b);
    assert_eq!(w.staking().get_allocated(&w.auditor), alloc_b);
    assert_eq!(w.staking().get_stake(&w.auditor), big_stake - alloc_a);
    assert_eq!(w.free_stake(), big_stake - alloc_a - alloc_b);
    assert_eq!(
        w.registry().get_certificate(&cert_b).status,
        CertStatus::Verified
    );
    assert_eq!(
        w.registry().get_certificate(&cert_a).status,
        CertStatus::Invalid
    );

    // B's collateral still unwinds normally once its own window closes.
    w.set_time(SETTLEMENT_DEADLINE);
    w.staking().release_allocation(&cert_b);
    assert_eq!(w.free_stake(), big_stake - alloc_a);
}

// ---------------------------------------------------------------------------
// 2d. Hygiene mode
// ---------------------------------------------------------------------------

/// A proof that is true but that nobody can evidence harm for.
///
/// The arbiter path is the live example: it carries a verdict, not a quantity,
/// so `raw_harm` is zero for every proof type it rules on. The certificate is
/// invalidated, the challenger is paid the flat bounty, and the operator's
/// reserve is not touched at all — no payout is invented from a number nobody
/// proved.
#[test]
fn hygiene_mode_kills_the_certificate_and_pays_a_flat_bounty_without_touching_the_reserve() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM); // honestly funded
    w.attest(cert_id);

    // Seed the bounty pool: the bounty is paid out of forfeited bonds and
    // nothing else, so a failed challenge has to have happened first.
    let dud = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(dud);
    assert!(w.cm().get_challenge(&dud).verdict == CmVerdict::ChallengeFails);
    assert_eq!(w.cm().get_bounty_pool(), MIN_CHALLENGE_STAKE);

    let operator_before = w.balance(&w.operator);
    let challenger_before = w.balance(&w.challenger);

    // A real covenant breach the arbiter confirms, but with no harm anybody
    // could evidence — the arbiter states zero. Hygiene mode stays reachable
    // through the arbiter path precisely because the quantity is theirs to set.
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &true, &0i128);

    // Certificate invalidated.
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
    assert!(!w.registry().verify(&w.agent).valid);

    // Challenger: bond back plus the flat bounty, and nothing proportional to
    // anybody's stake.
    assert_eq!(w.balance(&w.challenger), challenger_before + HYGIENE_BOUNTY);

    // Reserve untouched, operator untouched, victim paid nothing.
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.operator), operator_before);
    assert_eq!(w.balance(&w.victim), 0);

    // Auditor: not slashed, and the allocation retires so nothing is stranded
    // on the dead certificate.
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), 0);
}

/// The bounty can never exceed the forfeited-bond pool, and an empty pool pays
/// nothing rather than reaching for the reserve or the stake.
#[test]
fn hygiene_bounty_is_limited_to_the_forfeited_bond_pool() {
    let (w, cert_id) = staked_and_attested();
    assert_eq!(w.cm().get_bounty_pool(), 0);

    let challenger_before = w.balance(&w.challenger);
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &true, &0i128);

    // Bond back, bounty zero — nothing was invented out of the auditor's stake.
    assert_eq!(w.balance(&w.challenger), challenger_before);
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
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

/// **The post-expiry lock.**
///
/// A proof about activity *after* a certificate expires only becomes provable
/// after expiry. If the reserve unlocked at `expires_at` and the auditor's
/// allocation unlocked at the same instant, such a proof would settle against
/// an empty pot every single time: the operator would already have withdrawn
/// the reserve and the auditor would already have freed the stake.
///
/// So both stay locked until `expires_at + CHALLENGE_WINDOW`, and this test
/// walks the boundary from both sides.
#[test]
fn reserve_and_allocation_stay_locked_through_the_post_expiry_challenge_window() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);

    // Both pots agree on the deadline, because both read it from the Registry.
    assert_eq!(w.vault().get_unlock_at(&cert_id), SETTLEMENT_DEADLINE);
    assert_eq!(
        w.staking().allocation_unlock_at(&cert_id),
        SETTLEMENT_DEADLINE
    );
    assert_eq!(SETTLEMENT_DEADLINE, EXPIRES_AT + CHALLENGE_WINDOW);

    // Immediately after expiry — the moment a post-expiry proof first becomes
    // possible — neither side can walk away.
    for t in [EXPIRES_AT + 1, EXPIRES_AT + CHALLENGE_WINDOW / 2] {
        w.set_time(t);
        assert!(!w.registry().verify(&w.agent).valid); // expired...
        assert!(w.vault().try_release_to_operator(&cert_id).is_err());
        assert!(w.staking().try_release_allocation(&cert_id).is_err());
    }

    // The last locked second.
    w.set_time(SETTLEMENT_DEADLINE - 1);
    assert!(w.vault().try_release_to_operator(&cert_id).is_err());
    assert!(w.staking().try_release_allocation(&cert_id).is_err());

    // The money is all still exactly where it was, so a proof filed at this
    // instant would settle against a full pot.
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.free_stake(), 0);
    assert_eq!(w.balance(&w.operator), FUNDING - RESERVE_CLAIM);

    // At the deadline itself the lock lifts, and both unwind cleanly.
    w.set_time(SETTLEMENT_DEADLINE);
    w.vault().release_to_operator(&cert_id);
    w.staking().release_allocation(&cert_id);
    w.staking().release(&w.auditor);

    assert_eq!(w.balance(&w.operator), FUNDING);
    assert_eq!(w.balance(&w.auditor), FUNDING);
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert_eq!(w.balance(&w.vault), 0);
    assert_eq!(w.balance(&w.staking), 0);
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

    // Only the ChallengeManager may slash a certificate's allocation.
    assert!(w
        .staking()
        .try_slash_allocation(&cert_id, &w.challenger, &ALLOCATION)
        .is_err());
    // Only the ChallengeManager may retire an allocation early.
    assert!(w.staking().try_retire_allocation(&cert_id).is_err());
    // Only the ChallengeManager may pay out of a certificate's reserve — and
    // there is no entry point at all that pays the auditor's stake to a
    // caller-named address.
    assert!(w
        .vault()
        .try_pay_from_reserve(&cert_id, &w.challenger, &RESERVE_CLAIM)
        .is_err());
    // Only the ChallengeManager may kill a certificate.
    assert!(w.registry().try_invalidate(&cert_id).is_err());
    // Only the Registry may allocate an auditor's stake.
    assert!(w
        .staking()
        .try_allocate(&w.auditor, &cert_id, &ALLOCATION, &99_999u64)
        .is_err());
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
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.balance(&w.treasury), 0);
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
            args: (w.auditor.clone(), cert_id, ALLOCATION).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);

    assert!(w
        .registry()
        .try_attest(&w.auditor, &cert_id, &ALLOCATION)
        .is_err());
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
    assert!(w
        .registry()
        .try_attest(&w.auditor, &cert_id, &ALLOCATION)
        .is_err());
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

    // The fraud proof UPHOLDS, and this certificate's whole allocation is
    // slashed — to the treasury, which is the only place it may go.
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(w.staking().get_stake(&w.auditor), 0);
    assert_eq!(w.balance(&w.staking), 0);
    assert_eq!(w.balance(&w.treasury), ALLOCATION);
    // The named victim received nothing: this operator posted no reserve to
    // compensate anyone with, and the auditor's money is not theirs to receive.
    assert_eq!(w.balance(&w.victim), victim_before);
    assert_eq!(w.balance(&w.challenger), FUNDING);
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
    // ...and that unlock is the settlement deadline, not the expiry.
    assert_eq!(w.vault().get_unlock_at(&cert_a), SETTLEMENT_DEADLINE);
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

    // A's reserve pays the victim ($400, all of it) and the residual $200 of
    // harm is slashed from A's allocation to the treasury.
    assert_eq!(w.reserve_of(cert_a), 0);
    assert_eq!(w.balance(&w.victim), a_funding);
    assert_eq!(
        w.balance(&w.treasury),
        RESERVE_CLAIM - a_funding - a_funding
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
        + w.balance(&w.treasury)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 4);

    // B's operator can still reclaim it in full once B's challenge window shuts.
    w.set_time(SETTLEMENT_DEADLINE);
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
    w.set_time(SETTLEMENT_DEADLINE);
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

/// The arbiter path drives the **same waterfall** as the trustless one. The
/// only difference is where `harm` comes from: `resolve` computes it from chain
/// state, the arbiter states it.
///
/// That is not a new trust assumption. For these proof types the arbiter
/// already decides whether fraud occurred at all, so letting them also name the
/// amount grants them nothing they did not have — and their number travels the
/// identical rails: capped by `reserve + allocation`, victim paid only from the
/// operator's own reserve, slash only to the treasury.
#[test]
fn arbiter_resolves_subjective_proof_types_both_ways() {
    // Fraud found by the arbiter, with a stated harm → a real settlement.
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    let deposited = 100_0000000i128; // $100 of reserve, honestly declared or not
    w.deposit_reserve(cert_id, deposited);
    w.attest(cert_id);

    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    // The trustless resolver refuses: this proof type is not on-chain provable.
    assert!(w.cm().try_resolve(&id).is_err());

    // The arbiter rules that the agent blew through its bound, at a cost of $300.
    let stated_harm = 300_0000000i128;
    w.cm().resolve_by_arbiter(&id, &true, &stated_harm);
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeWins);

    //   victim = min(300, 100) = $100  <- the operator's own reserve, drained
    //   fee    = min(30, 0)    = $0    <- nothing left in that reserve
    //   slash  = 300 - 100     = $200  <- the treasury, capped by the $600 alloc
    let slash = stated_harm - deposited;
    assert_eq!(slash, 200_0000000);
    assert_eq!(w.balance(&w.victim), deposited);
    assert_eq!(w.balance(&w.treasury), slash);
    assert_eq!(w.reserve_of(cert_id), 0);

    // The auditor is slashed by exactly that, and the remainder comes back as
    // free stake rather than being stranded on the dead certificate.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE - slash);
    assert_eq!(w.free_stake(), ALLOCATION - slash);
    assert_eq!(w.free_stake(), 400_0000000);
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE - slash);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );

    // No fraud found → the challenger forfeits the bond, nothing else moves.
    let (w2, cert_id2) = staked_and_attested();
    let id2 = w2.challenge(cert_id2, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w2.cm().resolve_by_arbiter(&id2, &false, &0i128);
    assert!(w2.cm().get_challenge(&id2).verdict == CmVerdict::ChallengeFails);
    assert_eq!(w2.staking().get_stake(&w2.auditor), AUDITOR_STAKE);
    assert_eq!(w2.balance(&w2.victim), 0);
    assert_eq!(w2.balance(&w2.treasury), 0);
    assert_eq!(w2.balance(&w2.challenger), FUNDING - MIN_CHALLENGE_STAKE);

    // A rejected challenge carrying a non-zero harm is a contradiction, and is
    // refused rather than silently ignored.
    let (w3, cert_id3) = staked_and_attested();
    let id3 = w3.challenge(cert_id3, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    assert!(w3
        .cm()
        .try_resolve_by_arbiter(&id3, &false, &100_0000000i128)
        .is_err());
    // ...as is a negative one.
    assert!(w3
        .cm()
        .try_resolve_by_arbiter(&id3, &true, &-1i128)
        .is_err());
    assert!(w3.cm().get_challenge(&id3).verdict == CmVerdict::Pending);
    assert_eq!(w3.balance(&w3.treasury), 0);
}

/// **An arbiter cannot conjure money by overstating harm.**
///
/// The arbiter names a harm far larger than everything backing the certificate.
/// `payable = min(harm, reserve + allocation)` still binds: the victim gets the
/// whole reserve, the treasury gets the whole allocation, and the $98,900 of
/// "harm" beyond that is simply not paid — not borrowed from another
/// certificate, not taken from the auditor's unallocated stake, not minted.
#[test]
fn arbiter_stated_harm_beyond_the_collateral_is_still_capped_by_payable() {
    let w = BoundWorld::new();
    w.set_time(1_000);

    // A rich auditor, so there is plenty of unallocated stake to wrongly reach.
    let big_stake = 9_000_0000000i128;
    w.staking().stake(&w.auditor, &big_stake);

    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    let deposited = 400_0000000i128;
    w.deposit_reserve(cert_id, deposited);
    w.attest_with(cert_id, ALLOCATION); // $600 behind this certificate
    let unrelated = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(unrelated, RESERVE_CLAIM);

    let outrageous = 100_000_0000000i128; // $100,000
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &true, &outrageous);

    // payable = reserve + allocation = $400 + $600 = $1,000, and no more.
    assert_eq!(w.balance(&w.victim), deposited); // the whole reserve
    assert_eq!(w.balance(&w.treasury), ALLOCATION); // the whole allocation
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.allocation_of(cert_id), 0);

    // The excess came from nowhere, because there is nowhere for it to come
    // from: the auditor's unallocated stake is untouched...
    assert_eq!(w.staking().get_stake(&w.auditor), big_stake - ALLOCATION);
    assert_eq!(w.free_stake(), big_stake - ALLOCATION);
    // ...and so is an unrelated certificate's reserve in the same vault.
    assert_eq!(w.reserve_of(unrelated), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);

    // Conservation holds with a $100,000 claim against $1,000 of collateral.
    let total = w.balance(&w.operator)
        + w.balance(&w.operator2)
        + w.balance(&w.auditor)
        + w.balance(&w.challenger)
        + w.balance(&w.victim)
        + w.balance(&w.treasury)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 4);
}

/// **The arbiter cannot aim the slash.** Whatever verdict and quantity they
/// pass, slashed stake reaches the treasury and only the treasury — the
/// destination is not a parameter of any call the arbiter can make.
///
/// This is what keeps the self-dealing property intact on the arbiter path: an
/// arbiter who is bribed to overstate harm still cannot route the auditor's
/// money to whoever bribed them.
#[test]
fn arbiter_cannot_direct_the_slash_to_a_victim_or_a_challenger() {
    let (w, cert_id) = staked_and_attested(); // no reserve at all
    assert_eq!(w.reserve_of(cert_id), 0);

    // The challenger names themselves as the victim and the arbiter states a
    // harm far above the allocation — the most favourable possible setup for
    // the two parties who can actually influence the outcome.
    let id = w.cm().challenge(
        &w.challenger,
        &cert_id,
        &ProofType::BoundExceeded,
        &w.challenger,
        &MIN_CHALLENGE_STAKE,
    );
    w.cm().resolve_by_arbiter(&id, &true, &50_000_0000000i128);

    // Every slashed stroop is in the treasury.
    assert_eq!(w.balance(&w.treasury), ALLOCATION);
    // The challenger-cum-victim is exactly where they started: bond returned,
    // and no part of the auditor's stake. There was no reserve to compensate
    // them from, and the stake is not theirs to receive.
    assert_eq!(w.balance(&w.challenger), FUNDING);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.arbiter), 0);
    assert_eq!(
        w.staking().get_stake(&w.auditor),
        AUDITOR_STAKE - ALLOCATION
    );
}

/// Only the named arbiter may rule on a subjective challenge.
#[test]
fn arbiter_resolution_rejects_non_arbiter() {
    let (w, cert_id) = staked_and_attested();
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);

    w.env.set_auths(&[]);
    assert!(w.cm().try_resolve_by_arbiter(&id, &true, &0i128).is_err());
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
    assert!(w.cm().try_resolve_by_arbiter(&id, &true, &0i128).is_err());
    assert_eq!(w.balance(&w.victim), victim_after_first);
}

// ---------------------------------------------------------------------------
// 9. PaymentRouter — custody, metering, float cap and kill switch
// ---------------------------------------------------------------------------

/// The router's float cap for the standard certificate: $50.
const FLOAT_CAP: i128 = 50_0000000;
const DOLLAR: i128 = 1_0000000;

/// The standard world, with the agent enrolled in the router and holding
/// `float` of routed balance.
fn attested_and_enrolled(float: i128) -> (BoundWorld, u64) {
    let (w, cert_id) = staked_and_attested();
    w.mint(&w.agent, FUNDING);
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);
    w.router().deposit(&w.agent, &float);
    (w, cert_id)
}

/// Enrollment needs the agent **and** the operator, and the operator it needs is
/// the one the Registry names on that certificate — not a local admin field.
///
/// Either signature alone is refused. The operator's, because nobody may
/// conscript an address they do not control into someone else's metering and
/// kill switch. The agent's, because otherwise anyone could bind an address they
/// control to a stranger's certificate and manufacture spend evidence against
/// them.
#[test]
fn router_enrollment_requires_both_the_agent_and_the_certificates_operator() {
    let (w, cert_id) = staked_and_attested();

    let only = |who: &Address| {
        w.env.set_auths(&[]);
        w.env.mock_auths(&[MockAuth {
            address: who,
            invoke: &MockAuthInvoke {
                contract: &w.router,
                fn_name: "enroll",
                args: (w.agent.clone(), cert_id, FLOAT_CAP).into_val(&w.env),
                sub_invokes: &[],
            },
        }]);
    };

    only(&w.agent);
    assert!(w
        .router()
        .try_enroll(&w.agent, &cert_id, &FLOAT_CAP)
        .is_err());
    only(&w.operator);
    assert!(w
        .router()
        .try_enroll(&w.agent, &cert_id, &FLOAT_CAP)
        .is_err());
    // A different operator's signature is not the certificate's operator.
    only(&w.operator2);
    assert!(w
        .router()
        .try_enroll(&w.agent, &cert_id, &FLOAT_CAP)
        .is_err());
    assert!(!w.router().is_tracked(&w.agent));

    // Both together succeed.
    w.mock_all_auths();
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);
    assert!(w.router().is_tracked(&w.agent));
    assert_eq!(w.router().cert_of(&w.agent), Some(cert_id));
    assert_eq!(w.router().float_cap(&cert_id), FLOAT_CAP);

    // An agent that never enrolled is not tracked, even with a live certificate.
    assert!(!w.router().is_tracked(&w.agent2));
    assert_eq!(w.router().cert_of(&w.agent2), None);
}

/// The x402 settlement constraint, against the real deployment wiring: paying a
/// counterparty is one `transfer` call, one transfer event, no sub-invocation,
/// and no movement of the underlying USDC.
#[test]
fn router_transfer_is_a_single_flat_call_on_the_real_wiring() {
    let (w, _) = attested_and_enrolled(10_0000000);
    let usdc_in_custody = w.balance(&w.router);

    w.router().transfer(&w.agent, &w.victim, &4_0000000);

    // One transfer event plus the protocol's own spend event; exactly one of
    // them is a transfer, which is what a facilitator matches on.
    let events = w.env.events().all();
    assert_eq!(events.len(), 2);
    let transfers = events
        .iter()
        .filter(|(_, topics, _)| {
            Symbol::try_from_val(&w.env, &topics.get(0).unwrap()).unwrap()
                == symbol_short!("transfer")
        })
        .count();
    assert_eq!(transfers, 1);

    // Flat authorization tree: nothing was invoked underneath the transfer.
    let auths = w.env.auths();
    assert_eq!(auths.len(), 1);
    assert!(auths[0].1.sub_invocations.is_empty());

    // The underlying USDC never moved — custody is internal.
    assert_eq!(w.balance(&w.router), usdc_in_custody);
    assert_eq!(w.router().balance(&w.victim), 4_0000000);
    assert_eq!(w.router().total_supply(), w.balance(&w.router));
}

/// Metering follows enrollment, not the money. The tracked agent's payments
/// accumulate against its certificate; an untracked holder's do not accumulate
/// against anything.
#[test]
fn router_meters_only_enrolled_agents() {
    let (w, cert_id) = attested_and_enrolled(10_0000000);

    w.router().transfer(&w.agent, &w.victim, &3_0000000);
    w.router().transfer(&w.agent, &w.victim, &2_0000000);
    assert_eq!(w.router().spent(&cert_id), 5_0000000);

    // The victim now holds routed balance and pays it on. Nobody's certificate
    // is watching that address, so nobody's counter moves.
    w.router().transfer(&w.victim, &w.challenger, &DOLLAR);
    assert_eq!(w.router().spent(&cert_id), 5_0000000);

    // A second certificate on the same router keeps its own counter.
    let cert2 = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.mint(&w.agent2, FUNDING);
    w.router().enroll(&w.agent2, &cert2, &FLOAT_CAP);
    w.router().deposit(&w.agent2, &5_0000000);
    w.router().transfer(&w.agent2, &w.victim, &DOLLAR);
    assert_eq!(w.router().spent(&cert2), DOLLAR);
    assert_eq!(w.router().spent(&cert_id), 5_0000000);
}

/// **The $1 shuttle, on the real contracts.**
///
/// `contracts/spend-probe` proved that a cumulative spend counter measures gross
/// flow rather than loss. This reproduces that result against the router the
/// protocol will deploy, on a certificate the Registry actually issued and an
/// auditor actually attested.
///
/// One dollar of float shuttles between the enrolled agent and a second address
/// the operator controls. Every hop is a real, authorized, correctly recorded
/// payment. `spent` climbs past the certificate's own bound while the net flow
/// out of the operator's control is exactly zero — and the reserve, the auditor's
/// stake and the certificate's validity are all untouched.
///
/// If anyone ever wires `spent > bound` straight to a payout, this test is the
/// reason it must not ship.
#[test]
fn router_reproduces_the_dollar_shuttle_against_a_real_certificate() {
    // A deliberately tiny bound, so the shuttle is a handful of hops and the
    // test snapshot stays small. The economics are scale-free: at a $1,500 bound
    // it is 3,001 payments and a few dollars of fees.
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let small_bound = 10_0000000; // $10
    let cert_id = w.publish_cert(small_bound, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);
    assert!(w.registry().verify(&w.agent).valid);

    // `sink` is the operator's second address. The entire float is one dollar.
    let sink = w.operator2.clone();
    w.mint(&w.agent, DOLLAR);
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);
    w.router().deposit(&w.agent, &DOLLAR);

    let net_before = w.router().balance(&w.agent) + w.router().balance(&sink);

    let mut hops = 0i128;
    while w.router().spent(&cert_id) <= small_bound {
        if hops % 2 == 0 {
            w.router().transfer(&w.agent, &sink, &DOLLAR);
        } else {
            w.router().transfer(&sink, &w.agent, &DOLLAR);
        }
        hops += 1;
    }

    // The naive `BoundExceeded` predicate is now true.
    assert!(w.router().spent(&cert_id) > small_bound);
    assert_eq!(w.router().spent(&cert_id), 11_0000000); // $11 of "spend"
    assert_eq!(hops, 21); // 21 one-dollar payments

    // Net flow is exactly zero: the controlled pair holds what it started with.
    assert_eq!(
        w.router().balance(&w.agent) + w.router().balance(&sink),
        net_before
    );
    assert_eq!(net_before, DOLLAR);
    assert_eq!(w.balance(&w.router), DOLLAR); // custody never moved

    // Nobody outside the pair received anything.
    assert_eq!(w.router().balance(&w.victim), 0);
    assert_eq!(w.router().balance(&w.challenger), 0);

    // And the rest of the protocol is exactly as it was: the reserve is intact,
    // the auditor's stake is intact, the certificate is still valid. The counter
    // is evidence that the covenant was broken; it is not evidence of harm, and
    // there is no harm here to settle.
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert!(w.registry().verify(&w.agent).valid);
}

/// §6: the float cap bounds what a stolen agent key can reach. A deposit that
/// lands exactly on the cap is accepted; the next dollar is refused.
#[test]
fn router_float_cap_accepts_at_the_cap_and_refuses_beyond_it() {
    let (w, cert_id) = staked_and_attested();
    w.mint(&w.agent, FUNDING);
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);

    w.router().deposit(&w.agent, &FLOAT_CAP);
    assert_eq!(w.router().float(&cert_id), FLOAT_CAP);
    assert_eq!(w.balance(&w.router), FLOAT_CAP);

    let agent_usdc = w.balance(&w.agent);
    assert!(w.router().try_deposit(&w.agent, &DOLLAR).is_err());
    assert_eq!(w.router().float(&cert_id), FLOAT_CAP);
    assert_eq!(w.router().balance(&w.agent), FLOAT_CAP);
    assert_eq!(w.balance(&w.agent), agent_usdc);
    assert_eq!(w.balance(&w.router), FLOAT_CAP);

    // Whatever a thief does with the key, they cannot reach past the cap: the
    // most the router will ever hold for this certificate is $50.
    assert_eq!(w.router().float_cap(&cert_id), FLOAT_CAP);
}

/// §6 kill switch, end to end. The operator halts routing without a challenge;
/// the certificate stays Verified and valid, the auditor keeps their whole
/// stake, and the reserve is untouched. The agent key cannot clear the halt.
#[test]
fn router_kill_switch_halts_without_invalidating_or_slashing() {
    let (w, cert_id) = attested_and_enrolled(10_0000000);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.router().transfer(&w.agent, &w.victim, &DOLLAR);

    // Only the operator may halt.
    w.env.set_auths(&[]);
    assert!(w.router().try_halt(&cert_id).is_err());
    w.env.mock_auths(&[MockAuth {
        address: &w.agent,
        invoke: &MockAuthInvoke {
            contract: &w.router,
            fn_name: "halt",
            args: (cert_id,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.router().try_halt(&cert_id).is_err());

    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.operator,
        invoke: &MockAuthInvoke {
            contract: &w.router,
            fn_name: "halt",
            args: (cert_id,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    w.router().halt(&cert_id);
    assert!(w.router().is_halted(&cert_id));

    // Routing is dead even with the agent key fully authorized — which is the
    // situation a compromise response is actually in.
    w.mock_all_auths();
    assert!(w
        .router()
        .try_transfer(&w.agent, &w.victim, &DOLLAR)
        .is_err());
    assert!(w.router().try_withdraw(&w.agent, &DOLLAR).is_err());
    assert_eq!(w.router().spent(&cert_id), DOLLAR);
    assert_eq!(w.router().balance(&w.agent), 9_0000000);

    // Halting is not a challenge: nothing about the certificate or the auditor
    // changed.
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    assert!(w.registry().verify(&w.agent).valid);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);

    // A thief holding the agent key cannot re-enable routing.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.agent,
        invoke: &MockAuthInvoke {
            contract: &w.router,
            fn_name: "resume",
            args: (cert_id,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.router().try_resume(&cert_id).is_err());
    assert!(w.router().is_halted(&cert_id));

    // The operator can.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.operator,
        invoke: &MockAuthInvoke {
            contract: &w.router,
            fn_name: "resume",
            args: (cert_id,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    w.router().resume(&cert_id);
    assert!(!w.router().is_halted(&cert_id));

    w.mock_all_auths();
    w.router().transfer(&w.agent, &w.victim, &DOLLAR);
    assert_eq!(w.router().spent(&cert_id), 2 * DOLLAR);
}

/// The hole a halt would otherwise leave open: an allowance the thief granted
/// themselves *before* the halt.
///
/// While halted, `transfer_from` against the certificate's agent is refused too.
/// The allowance is suspended, not deleted — a halt/resume cycle must not
/// silently destroy a legitimate standing approval.
#[test]
fn router_halt_suspends_pre_existing_allowances_without_destroying_them() {
    let (w, cert_id) = attested_and_enrolled(10_0000000);
    let thief = w.challenger.clone();

    w.router().approve(&w.agent, &thief, &6_0000000, &1_000u32);
    assert_eq!(w.router().allowance(&w.agent, &thief), 6_0000000);

    w.router().halt(&cert_id);

    assert!(w
        .router()
        .try_transfer_from(&thief, &w.agent, &thief, &4_0000000)
        .is_err());
    assert_eq!(w.router().balance(&w.agent), 10_0000000);
    assert_eq!(w.router().balance(&thief), 0);
    assert_eq!(w.router().spent(&cert_id), 0);
    assert_eq!(w.router().allowance(&w.agent, &thief), 6_0000000);

    w.router().resume(&cert_id);

    w.router()
        .transfer_from(&thief, &w.agent, &thief, &4_0000000);
    assert_eq!(w.router().balance(&w.agent), 6_0000000);
    assert_eq!(w.router().balance(&thief), 4_0000000);
    assert_eq!(w.router().allowance(&w.agent, &thief), 2_0000000);
    assert_eq!(w.router().spent(&cert_id), 4_0000000);
}

/// §7: post-expiry payments are recorded separately, with enough detail for a
/// grace window and a de-minimis floor to be applied by a predicate later — and
/// they still count as spend, because they are still flow.
#[test]
fn router_records_post_expiry_payments_without_judging_them() {
    let (w, cert_id) = attested_and_enrolled(10_0000000);

    w.router().transfer(&w.agent, &w.victim, &2_0000000);
    assert_eq!(w.router().post_expiry_spent(&cert_id).count, 0);

    // The certificate expires. The Registry stops calling it valid...
    w.set_time(EXPIRES_AT + 1);
    assert!(!w.registry().verify(&w.agent).valid);

    // ...but the router keeps routing and keeps counting, because refusing to
    // record is not the same as refusing to pay, and the predicate that decides
    // what a late payment means does not live here.
    w.router().transfer(&w.agent, &w.victim, &DOLLAR);
    w.set_time(EXPIRES_AT + 100_000);
    w.router().transfer(&w.agent, &w.victim, &3_0000000);

    let pe = w.router().post_expiry_spent(&cert_id);
    assert_eq!(pe.total, 4_0000000);
    assert_eq!(pe.count, 2);
    // The grace window is measured from the first late payment...
    assert_eq!(pe.first_at, EXPIRES_AT + 1);
    // ...and the de-minimis floor is applied to the largest one.
    assert_eq!(pe.max_payment, 3_0000000);
    assert_eq!(pe.max_payment_at, EXPIRES_AT + 100_000);

    // Post-expiry flow is still flow.
    assert_eq!(w.router().spent(&cert_id), 6_0000000);

    // Neither the grace window nor the floor has been applied by the router: a
    // one-dollar payment one second after expiry was recorded in full.
    assert_eq!(w.router().post_expiry_spent(&cert_id).total, 4_0000000);
}
