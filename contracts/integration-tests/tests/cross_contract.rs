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
use premium_vault::{PremiumVault, PremiumVaultClient, SECONDS_PER_YEAR};
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
/// DESIGN-V2 §1's claim window: 72 hours. A first valid challenge opens one
/// instead of settling, and settlement runs once, at its close, over every
/// claim it admitted. Tests that care about the exact ledger instant of
/// settlement file their claim `CLAIM_WINDOW` seconds ahead of it.
const CLAIM_WINDOW: u64 = 72 * 60 * 60;
/// The challenger's fee, in basis points of proven harm (10%).
const CHALLENGER_FEE_BPS: i128 = 1_000;
/// The flat hygiene bounty the ChallengeManager pays when a proof is true but
/// no harm is evidenced.
const HYGIENE_BOUNTY: i128 = 10_0000000;
/// The annualised coverage rate the PremiumVault is deployed with: 200 bps, 2%
/// of the bound per year.
const RATE_BPS: i128 = 200;
/// The protocol's share of each premium: 10%.
const PREMIUM_FEE_BPS: i128 = 1_000;

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
    premium_vault: Address,

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
    /// A second, unrelated claimant pair. DESIGN-V2 §1's claim window
    /// aggregates many claims against one certificate, so the harness needs
    /// more than one of each.
    challenger2: Address,
    victim2: Address,
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
        let challenger2 = Address::generate(&env);
        let victim2 = Address::generate(&env);
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
        let premium_vault = env.register(PremiumVault, ());

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
        PremiumVaultClient::new(&env, &premium_vault).initialize(
            &registry,
            &challenge_manager,
            &token,
            &treasury,
            &RATE_BPS,
            &PREMIUM_FEE_BPS,
        );
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
        // The router is wired in a second, arbiter-authorized one-shot call
        // rather than as a ninth `initialize` argument — see `set_router`.
        ChallengeManagerClient::new(&env, &challenge_manager).set_router(&router);
        // Same shape, same trap: step 4 of the waterfall is silently skipped on
        // a deployment that never makes this call.
        ChallengeManagerClient::new(&env, &challenge_manager).set_premium_vault(&premium_vault);

        let world = Self {
            env,
            token,
            registry,
            vault,
            staking,
            escrow,
            challenge_manager,
            router,
            premium_vault,
            token_admin,
            operator,
            agent,
            operator2,
            agent2,
            auditor,
            challenger,
            victim,
            challenger2,
            victim2,
            arbiter,
            treasury,
        };

        for who in [
            world.operator.clone(),
            world.operator2.clone(),
            world.auditor.clone(),
            world.challenger.clone(),
            world.challenger2.clone(),
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
    fn premium(&self) -> PremiumVaultClient<'_> {
        PremiumVaultClient::new(&self.env, &self.premium_vault)
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

    /// Take money back out of a certificate's reserve, to its operator.
    ///
    /// DESIGN-V2 §4. `attest` now refuses a certificate whose reserve is not
    /// funded, so a scenario that needs an underfunded *attested* certificate
    /// can no longer simply skip the deposit — it has to reach that state the
    /// way the real world does: fund honestly, get attested, then pull the
    /// money back out. Attestation-time fraud is exactly what §4 closed;
    /// post-attestation drawdown is exactly what it did **not**, and is what
    /// `InsufficientReserve` exists to catch. Every shortfall scenario below
    /// therefore now models the case that is still live rather than the one
    /// that is not.
    ///
    /// The drawdown goes through `pay_from_reserve`, which is
    /// ChallengeManager-only on the live network and is reachable here only
    /// because the harness mocks auths. It stands in for the slower real
    /// sequence — wait out the settlement deadline, then `release_to_operator`
    /// — because that sequence would move every scenario's clock past expiry
    /// and change what the rest of each test is measuring. The end state is
    /// identical: the certificate is attested and its vault is short.
    fn withdraw_reserve_to(&self, cert_id: u64, to: &Address, amount: i128) {
        if amount > 0 {
            self.vault().pay_from_reserve(&cert_id, to, &amount);
        }
    }

    /// Fund a certificate's reserve in full, attest it, then leave exactly
    /// `remaining` behind — the post-§4 way to build a certificate that is
    /// attested and short. `claim` is the reserve the certificate advertises.
    fn attest_with_reserve(&self, cert_id: u64, claim: i128, remaining: i128) {
        self.attest_with_reserve_and(cert_id, claim, remaining, ALLOCATION);
    }

    /// As `attest_with_reserve`, naming the allocation explicitly.
    fn attest_with_reserve_and(
        &self,
        cert_id: u64,
        claim: i128,
        remaining: i128,
        allocation: i128,
    ) {
        self.attest_with_reserve_for(cert_id, &self.operator, claim, remaining, allocation);
    }

    /// The general form: the certificate's own operator funds it and takes the
    /// surplus back, so a second operator's certificate nets out against the
    /// right account and the conservation checks stay exact.
    fn attest_with_reserve_for(
        &self,
        cert_id: u64,
        operator: &Address,
        claim: i128,
        remaining: i128,
        allocation: i128,
    ) {
        self.deposit_reserve(cert_id, claim);
        self.attest_with(cert_id, allocation);
        self.withdraw_reserve_to(cert_id, operator, claim - remaining);
    }

    fn reserve_of(&self, cert_id: u64) -> i128 {
        self.vault().get_balance(&cert_id)
    }

    /// The operator buys coverage for a certificate. Must follow `attest`: the
    /// premium is yield on staked capital, so there has to be an auditor with
    /// an allocation behind the certificate first.
    fn pay_premium(&self, cert_id: u64) {
        self.premium().pay_premium(&cert_id);
    }

    fn premium_of(&self, cert_id: u64) -> i128 {
        self.premium().get_premium(&cert_id)
    }

    fn accrued(&self, cert_id: u64) -> i128 {
        self.premium().accrued(&cert_id)
    }

    fn claim_premium(&self, cert_id: u64) -> i128 {
        self.premium().claim(&cert_id)
    }

    /// Every account that can hold money in these scenarios. Used by the
    /// conservation checks, which now have to include the premium vault.
    fn total_in_the_system(&self) -> i128 {
        self.balance(&self.operator)
            + self.balance(&self.operator2)
            + self.balance(&self.auditor)
            + self.balance(&self.challenger)
            + self.balance(&self.victim)
            + self.balance(&self.challenger2)
            + self.balance(&self.victim2)
            + self.balance(&self.treasury)
            + self.balance(&self.vault)
            + self.balance(&self.staking)
            + self.balance(&self.challenge_manager)
            + self.balance(&self.premium_vault)
    }

    fn deposit_fee(&self, amount: i128) {
        self.escrow()
            .deposit(&self.operator, &self.auditor, &amount);
    }

    fn challenge(&self, cert_id: u64, proof: ProofType, bond: i128) -> u64 {
        self.cm()
            .challenge(&self.challenger, &cert_id, &proof, &self.victim, &bond)
    }

    /// File a claim naming an arbitrary challenger and victim. A claim window
    /// admits claims from unrelated parties, so most §1 tests need this.
    fn challenge_as(
        &self,
        challenger: &Address,
        victim: &Address,
        cert_id: u64,
        proof: ProofType,
        bond: i128,
    ) -> u64 {
        self.cm()
            .challenge(challenger, &cert_id, &proof, victim, &bond)
    }

    /// Restore blanket auth mocking after a test has narrowed it with
    /// `mock_auths`.
    fn mock_all_auths(&self) {
        self.env.mock_all_auths();
    }

    /// Close the claim window this challenge belongs to, and settle it.
    ///
    /// DESIGN-V2 §1 replaced per-challenge resolution with a per-certificate
    /// claim window, so "resolving" is now: wait out the window, then close it
    /// once over every claim it admitted. The ledger clock is moved to the
    /// close instant, which is where the extra 72 hours in most of these tests
    /// comes from.
    ///
    /// A claim that was **false at filing** never opened a window — it was
    /// rejected on the spot inside `challenge()` — so there is nothing to close
    /// and this returns without doing anything.
    fn resolve(&self, challenge_id: u64) {
        let cert_id = self.cm().get_challenge(&challenge_id).cert_id;
        self.settle_window(cert_id);
    }

    /// Advance to the window's close and settle. Permissionless: any address
    /// may make this call, and here the test harness makes it.
    fn settle_window(&self, cert_id: u64) {
        let closes_at = self.cm().window_closes_at(&cert_id);
        if closes_at == 0 {
            return;
        }
        if self.env.ledger().timestamp() < closes_at {
            self.set_time(closes_at);
        }
        self.cm().close_window(&cert_id);
    }

    fn verdict_of(&self, challenge_id: u64) -> CmVerdict {
        self.cm().get_challenge(&challenge_id).verdict
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
    // Funded, attested, and then emptied. DESIGN-V2 §4 means the reserve has to
    // be there at attestation, so the empty vault these scenarios rely on is
    // now reached by the operator withdrawing it afterwards — see
    // `withdraw_reserve`. The end state is what it always was: an attested
    // certificate with nothing behind it.
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, 0);
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
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, deposited);

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
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, deposited);

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
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, deposited);

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
    w.attest_with_reserve_and(cert_id, claim, 0, ALLOCATION);
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
    w.attest_with_reserve_and(cert_id, RESERVE_CLAIM, deposited, big_allocation);
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
    w.attest_with_reserve_and(cert_id, tiny_claim, 0, big_allocation);

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
    w.attest_with_reserve_and(cert_a, RESERVE_CLAIM, 0, alloc_a);
    w.attest_with_reserve_for(cert_b, &w.operator2, RESERVE_CLAIM, 0, alloc_b);

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
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &true, &0i128);
    w.settle_window(cert_id);

    // Certificate invalidated.
    assert!(w.verdict_of(id) == CmVerdict::ChallengeWins);
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
    w.settle_window(cert_id);
    assert!(w.verdict_of(id) == CmVerdict::ChallengeWins);

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

// ---------------------------------------------------------------------------
// DESIGN-V2 §4 — attest verifies the reserve
// ---------------------------------------------------------------------------

/// **The griefing sequence fails at step two.**
///
/// This is the whole of §4, run end to end. The operator publishes a
/// certificate claiming a reserve it has not posted, and waits for an auditor.
/// Before this fix the auditor could sign it, and the operator could then
/// challenge its own certificate and burn the auditor's entire allocation to
/// the treasury for the price of gas.
///
/// Now attestation is refused, and every later step has nothing to work with:
/// the certificate is still `Pending`, no allocation was ever taken from the
/// auditor, and the self-challenge finds no attestation to slash.
#[test]
fn an_underfunded_certificate_cannot_be_attested_and_so_cannot_grief_an_auditor() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);

    // Step one: publish a certificate claiming $1,000 of reserve, and post
    // nothing at all behind it.
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    assert_eq!(w.reserve_of(cert_id), 0);

    // Step two: the auditor is walked onto it — and the Registry refuses.
    assert!(
        w.registry()
            .try_attest(&w.auditor, &cert_id, &ALLOCATION)
            .is_err(),
        "attest must refuse a certificate whose reserve is not funded"
    );

    // Nothing was recorded. The certificate never became `Verified`, it never
    // acquired an auditor, and — the part that matters — the auditor's stake is
    // entirely free. There is no allocation to lose.
    let cert = w.registry().get_certificate(&cert_id);
    assert!(cert.status == CertStatus::Pending);
    assert!(cert.auditor.is_none());
    assert_eq!(cert.auditor_stake_snapshot, 0);
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert!(!w.registry().verify(&w.agent).valid);

    // Step three: the operator self-challenges anyway. The shortfall is real —
    // the vault is empty against a $1,000 claim — so the claim is true at
    // filing and opens a window. It settles against an unattested certificate,
    // which is to say against nothing: there is no allocation to slash.
    let auditor_before = w.staking().get_stake(&w.auditor);
    let treasury_before = w.balance(&w.treasury);
    let challenge_id = w.challenge_as(
        &w.operator,
        &w.operator2,
        cert_id,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    w.resolve(challenge_id);

    // The auditor is untouched — the entire point of §4.
    assert_eq!(w.staking().get_stake(&w.auditor), auditor_before);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.treasury), treasury_before);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);
}

/// **Post-attestation withdrawal remains slashable.**
///
/// §4 closes the attestation-time trap and nothing more. The operator funds the
/// reserve honestly, the auditor attests against a full vault, and then the
/// operator takes the money back out through the only legitimate route there
/// is: waiting out the certificate's settlement deadline and calling
/// `release_to_operator`. The `InsufficientReserve` proof still upholds against
/// what is left, and the auditor's allocation is still slashed.
///
/// This is the residue §4 explicitly does not cover, demonstrated rather than
/// asserted in prose — and it is why the proof and the auditor's own monitoring
/// still have work to do after this change.
#[test]
fn a_reserve_withdrawn_after_attestation_is_still_provable_fraud() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);

    // Funded in full, and attested against a vault that really holds the claim.
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert!(w.registry().verify(&w.agent).valid);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);

    // The reserve is locked until the settlement deadline, so the operator has
    // to wait it out. That lock is the reason this is the *only* way the money
    // leaves, and it is what makes §4 stronger in practice than §4 claims.
    assert!(w.vault().is_locked(&cert_id));
    w.set_time(SETTLEMENT_DEADLINE);
    w.vault().release_to_operator(&cert_id);
    assert_eq!(w.reserve_of(cert_id), 0);

    // The certificate still advertises a $1,000 reserve it no longer holds.
    assert_eq!(w.registry().get_cert_reserve(&cert_id), RESERVE_CLAIM);

    // The proof upholds, and the auditor pays for it. The allocation has passed
    // its unlock instant but the auditor never withdrew it, so it is still
    // there to be slashed — which is precisely the exposure §4 leaves open.
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(w.balance(&w.treasury), ALLOCATION);
    assert_eq!(
        w.staking().get_stake(&w.auditor),
        AUDITOR_STAKE - ALLOCATION
    );
    assert_eq!(w.allocation_of(cert_id), 0);
    assert!(
        w.registry().get_certificate(&cert_id).status == CertStatus::Invalid,
        "the certificate is dead"
    );
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
    // A is under-funded (the fraud); B is fully funded and innocent. A is
    // attested against a full reserve and then drawn down — DESIGN-V2 §4.
    let a_funding = 400_0000000i128;
    w.attest_with_reserve(cert_a, RESERVE_CLAIM, a_funding);
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
        + w.balance(&w.challenger2)
        + w.balance(&w.victim2)
        + w.balance(&w.treasury)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 5);

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
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, deposited);

    // `FakeSignature` is the proof type no contract can verify: it is filed
    // un-adjudicated and waits for the arbiter. A `BoundExceeded` claim on a
    // certificate whose counter has not moved is *false at filing* and is
    // rejected inside `challenge()` — the router is the source of truth for
    // what it measures, and the arbiter may not overturn it.
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    // The window is open and cannot be closed early.
    assert!(w.cm().try_close_window(&cert_id).is_err());

    // The arbiter rules that the auditor's signature was forged, at a cost of
    // $300. That records the verdict and the quantity; the money moves at
    // window close, over the whole window.
    let stated_harm = 300_0000000i128;
    w.cm().resolve_by_arbiter(&id, &true, &stated_harm);
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::Pending);
    w.settle_window(cert_id);
    assert!(w.verdict_of(id) == CmVerdict::ChallengeWins);

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
    w2.settle_window(cert_id2);
    assert!(w2.verdict_of(id2) == CmVerdict::ChallengeFails);
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
    // $600 behind this certificate, against a reserve funded then drawn down.
    w.attest_with_reserve_and(cert_id, RESERVE_CLAIM, deposited, ALLOCATION);
    let unrelated = w.publish_cert_for(&w.operator2, &w.agent2, BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(unrelated, RESERVE_CLAIM);

    let outrageous = 100_000_0000000i128; // $100,000
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &true, &outrageous);
    w.settle_window(cert_id);

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
        + w.balance(&w.challenger2)
        + w.balance(&w.victim2)
        + w.balance(&w.treasury)
        + w.balance(&w.vault)
        + w.balance(&w.staking)
        + w.balance(&w.challenge_manager);
    assert_eq!(total, FUNDING * 5);
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
        &ProofType::FakeSignature,
        &w.challenger,
        &MIN_CHALLENGE_STAKE,
    );
    w.cm().resolve_by_arbiter(&id, &true, &50_000_0000000i128);
    w.settle_window(cert_id);

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
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);

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

    assert!(w.cm().try_close_window(&cert_id).is_err());
    assert!(w.cm().try_resolve_by_arbiter(&id, &true, &0i128).is_err());
    // And no second window may ever open on a settled certificate.
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

// ---------------------------------------------------------------------------
// 10. Trustless predicates: BoundExceeded and ExpiredCertificate
//
// Both are proven from router state alone, and both settle in HYGIENE MODE:
// the certificate is invalidated, the challenger is paid the flat bounty out of
// forfeited bonds, the operator's reserve is not touched, and THE AUDITOR IS
// NOT SLASHED.
//
// That last clause is the security property these tests exist to pin down, not
// a missing feature. Both predicates are manufacturable at will by the
// operator — the shuttle test below does exactly that — so a slash on the
// counter alone would let any operator destroy their auditor's allocation for
// the price of gas. See the comment on `ChallengeManager::resolve`.
// ---------------------------------------------------------------------------

/// The grace window the ChallengeManager applies to post-expiry payments: 24h.
const GRACE_WINDOW: u64 = 24 * 60 * 60;
/// The de-minimis floor, in basis points of the certificate's own bound: 0.1%.
const DE_MINIMIS_FLOOR_BPS: i128 = 10;
/// A $10 bound, small enough that a handful of dollar payments clears it.
const SMALL_BOUND: i128 = 10_0000000;

fn floor_for(bound: i128) -> i128 {
    bound * DE_MINIMIS_FLOOR_BPS / 10_000
}

/// Staked, published, funded, attested and enrolled in the router, on a
/// caller-chosen bound. The reserve is honestly funded throughout, so no
/// `InsufficientReserve` proof is lurking in any of these tests.
fn predicate_world(bound: i128) -> (BoundWorld, u64) {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(bound, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);
    w.mint(&w.agent, FUNDING);
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);
    w.router().deposit(&w.agent, &FLOAT_CAP);
    (w, cert_id)
}

/// The hygiene bounty comes out of forfeited bonds and nothing else, so a
/// failed challenge has to have happened first. The certificate is honestly
/// funded, so an `InsufficientReserve` challenge against it fails and its bond
/// is forfeited.
fn seed_bounty_pool(w: &BoundWorld, cert_id: u64) {
    let dud = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(dud);
    assert!(w.cm().get_challenge(&dud).verdict == CmVerdict::ChallengeFails);
    assert_eq!(w.cm().get_bounty_pool(), MIN_CHALLENGE_STAKE);
}

/// Custody is never fractional: the router's internal supply always equals the
/// underlying USDC it holds.
fn assert_router_fully_backed(w: &BoundWorld) {
    assert_eq!(w.router().total_supply(), w.balance(&w.router));
}

/// Everything the auditor owns, unchanged to the stroop.
///
/// The allocation retires to zero — that is the point of retiring it, and the
/// capital comes straight back as free stake — so the invariant that matters is
/// that the custodied USDC never moved and the treasury never received a
/// stroop.
fn assert_auditor_untouched(w: &BoundWorld) {
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.treasury), 0);
}

// --- BoundExceeded ---------------------------------------------------------

/// `spent > bound` is proven by `resolve` from the router's counter alone — no
/// arbiter — and settles as hygiene.
#[test]
fn bound_exceeded_is_provable_from_router_state_and_settles_as_hygiene() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);
    seed_bounty_pool(&w, cert_id);

    // $11 of routed flow against a $10 bound.
    w.router().transfer(&w.agent, &w.victim, &11_0000000);
    assert_eq!(w.router().spent(&cert_id), 11_0000000);
    assert!(w.router().spent(&cert_id) > SMALL_BOUND);

    let challenger_before = w.balance(&w.challenger);
    let operator_before = w.balance(&w.operator);

    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.resolve(id); // the trustless path, not the arbiter

    // The certificate is dead.
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
    assert!(!w.registry().verify(&w.agent).valid);

    // The challenger got the flat bounty and nothing proportional to anyone's
    // stake: bond back, plus $10.
    assert_eq!(w.balance(&w.challenger), challenger_before + HYGIENE_BOUNTY);

    // The auditor is untouched, and the allocation retired rather than being
    // stranded on a dead certificate.
    assert_auditor_untouched(&w);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), 0);

    // The reserve was never opened.
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.vault), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.operator), operator_before);
    assert_eq!(w.balance(&w.victim), 0);
    assert_router_fully_backed(&w);
}

/// **The headline test: the $1 shuttle drives a real, trustlessly-provable
/// `BoundExceeded`, and it costs the auditor nothing.**
///
/// One dollar of float shuttles between the enrolled agent and a second address
/// the operator controls. Every hop is a real, authorized, correctly recorded
/// payment; net flow out of the operator's control is exactly zero. The counter
/// clears the bound, `resolve` upholds the proof with no arbiter involved, and
/// the certificate dies.
///
/// And the auditor's capital does not move by one stroop. If it did, this
/// sequence — gas and nothing else — would be a way for any operator to destroy
/// their auditor's allocation on demand, and nobody would ever audit anything.
#[test]
fn the_dollar_shuttle_proves_bound_exceeded_and_costs_the_auditor_nothing() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(SMALL_BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);
    seed_bounty_pool(&w, cert_id);

    // `sink` is the operator's own second address. The entire float is $1.
    let sink = w.operator2.clone();
    w.mint(&w.agent, DOLLAR);
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);
    w.router().deposit(&w.agent, &DOLLAR);

    let mut hops = 0i128;
    while w.router().spent(&cert_id) <= SMALL_BOUND {
        if hops % 2 == 0 {
            w.router().transfer(&w.agent, &sink, &DOLLAR);
        } else {
            w.router().transfer(&sink, &w.agent, &DOLLAR);
        }
        hops += 1;
    }
    assert_eq!(hops, 21);
    assert_eq!(w.router().spent(&cert_id), 11_0000000);

    // Net flow is exactly zero: the controlled pair holds what it started with,
    // and custody never moved.
    assert_eq!(
        w.router().balance(&w.agent) + w.router().balance(&sink),
        DOLLAR
    );
    assert_eq!(w.balance(&w.router), DOLLAR);
    assert_eq!(w.router().balance(&w.victim), 0);

    let challenger_before = w.balance(&w.challenger);
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    // The manufactured proof is a real proof, and it kills the certificate.
    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );

    // And it costs the auditor nothing at all.
    assert_auditor_untouched(&w);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), 0);

    // Nor the operator's reserve — the operator pays with their own dead
    // certificate, which is the whole penalty and the correct one.
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.challenger), challenger_before + HYGIENE_BOUNTY);
    assert_router_fully_backed(&w);
}

/// The predicate is `spent > bound`, not `spent >= bound`. Spending exactly the
/// bound is the covenant being kept, and the challenge fails with every balance
/// where it was.
#[test]
fn bound_exceeded_is_rejected_when_spend_is_within_the_bound() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);

    w.router().transfer(&w.agent, &w.victim, &SMALL_BOUND);
    assert_eq!(w.router().spent(&cert_id), SMALL_BOUND);

    let auditor_before = w.balance(&w.auditor);
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeFails);
    assert!(w.registry().verify(&w.agent).valid);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );

    // Nothing moved but the forfeited bond.
    assert_auditor_untouched(&w);
    assert_eq!(w.balance(&w.auditor), auditor_before);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_router_fully_backed(&w);
}

// --- ExpiredCertificate (DESIGN-V2 §7) -------------------------------------

/// All three §7 conditions hold: the payment settled after the grace window,
/// it cleared the de-minimis floor, and the certificate was neither renewed nor
/// already invalid. Upheld by `resolve`, settled as hygiene.
#[test]
fn expired_certificate_upholds_when_all_three_conditions_hold() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);
    seed_bounty_pool(&w, cert_id);

    // A dollar is far above 0.1% of a $10 bound.
    assert!(DOLLAR >= floor_for(SMALL_BOUND));

    w.set_time(EXPIRES_AT + GRACE_WINDOW + 1);
    w.router().transfer(&w.agent, &w.victim, &DOLLAR);

    let pe = w.router().post_expiry_spent(&cert_id);
    assert_eq!(pe.count, 1);
    assert_eq!(pe.max_payment, DOLLAR);
    assert_eq!(pe.max_payment_at, EXPIRES_AT + GRACE_WINDOW + 1);

    let challenger_before = w.balance(&w.challenger);
    let id = w.challenge(cert_id, ProofType::ExpiredCertificate, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );

    // Hygiene, exactly as for BoundExceeded: bounty paid, reserve untouched,
    // auditor whole.
    assert_eq!(w.balance(&w.challenger), challenger_before + HYGIENE_BOUNTY);
    assert_auditor_untouched(&w);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.victim), 0);
    assert_router_fully_backed(&w);
}

/// Condition 1 fails: the late payment landed inside the grace window.
///
/// This is the $1-post-expiry attack §7 exists to defuse — a hostile
/// counterparty invoicing an agent moments after expiry must not be able to
/// kill an honest certificate.
#[test]
fn expired_certificate_is_rejected_inside_the_grace_window() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);

    // Late, well above the floor, but comfortably inside 24 hours — and the
    // boundary itself is inside: the predicate wants strictly after.
    w.set_time(EXPIRES_AT + GRACE_WINDOW);
    w.router().transfer(&w.agent, &w.victim, &5_0000000);
    assert_eq!(w.router().post_expiry_spent(&cert_id).count, 1);

    let id = w.challenge(cert_id, ProofType::ExpiredCertificate, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeFails);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    assert_auditor_untouched(&w);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_router_fully_backed(&w);
}

/// Condition 2 fails: the late payment is below 0.1% of the bound.
#[test]
fn expired_certificate_is_rejected_below_the_de_minimis_floor() {
    let (w, cert_id) = predicate_world(BOUND); // $5,000 bound → a $5 floor
    assert_eq!(floor_for(BOUND), 5_0000000);

    w.set_time(EXPIRES_AT + GRACE_WINDOW + 1);
    w.router().transfer(&w.agent, &w.victim, &DOLLAR);
    assert!(w.router().post_expiry_spent(&cert_id).max_payment < floor_for(BOUND));

    let id = w.challenge(cert_id, ProofType::ExpiredCertificate, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeFails);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    assert_auditor_untouched(&w);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_router_fully_backed(&w);
}

/// **The floor scales with the bound.** The identical $1 late payment, at the
/// identical instant, is below the floor on a $5,000 bound and above it on a
/// $10 one. A flat floor would be irrelevant at the first and fatal at the
/// second, which is why §7 makes it a percentage.
#[test]
fn the_de_minimis_floor_scales_with_the_certificates_bound() {
    let late = |bound: i128| -> CmVerdict {
        let (w, cert_id) = predicate_world(bound);
        w.set_time(EXPIRES_AT + GRACE_WINDOW + 1);
        w.router().transfer(&w.agent, &w.victim, &DOLLAR);
        let id = w.challenge(cert_id, ProofType::ExpiredCertificate, MIN_CHALLENGE_STAKE);
        w.resolve(id);
        w.cm().get_challenge(&id).verdict
    };

    assert!(DOLLAR < floor_for(BOUND));
    assert!(DOLLAR > floor_for(SMALL_BOUND));
    assert!(late(BOUND) == CmVerdict::ChallengeFails);
    assert!(late(SMALL_BOUND) == CmVerdict::ChallengeWins);
}

/// Condition 3 fails: the operator renewed. In this registry renewal means
/// publishing a fresh certificate for the same agent, which re-points the
/// agent's mapping — so the old certificate's late activity is no longer
/// evidence against a live covenant.
#[test]
fn expired_certificate_is_rejected_when_the_certificate_was_renewed() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);

    w.set_time(EXPIRES_AT + GRACE_WINDOW + 1);
    w.router().transfer(&w.agent, &w.victim, &DOLLAR);
    assert!(w.router().post_expiry_spent(&cert_id).max_payment >= floor_for(SMALL_BOUND));

    // The operator renews: a new certificate for the same agent.
    let renewed = w.publish_cert(
        SMALL_BOUND,
        RESERVE_CLAIM,
        EXPIRES_AT + 10 * CHALLENGE_WINDOW,
    );
    assert!(renewed != cert_id);
    assert_eq!(w.registry().get_cert_id(&w.agent), renewed);

    let id = w.challenge(cert_id, ProofType::ExpiredCertificate, MIN_CHALLENGE_STAKE);
    w.resolve(id);

    assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeFails);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    assert_auditor_untouched(&w);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_router_fully_backed(&w);
}

// --- Neither proof is available against an unwatched agent -----------------

/// §8's point, restated as a settlement property: an agent that never enrolled
/// meters nothing, so neither predicate can ever be true against its
/// certificate — not even when it is demonstrably spending past its bound and
/// long past its expiry.
///
/// That is not a hole a challenger can exploit; it is the reason enrollment
/// needs both signatures. Nobody can attach spend to a stranger's certificate.
#[test]
fn an_untracked_agent_can_produce_neither_proof() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(SMALL_BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.deposit_reserve(cert_id, RESERVE_CLAIM);
    w.attest(cert_id);

    // The agent holds routed balance and pays it away, well past expiry and far
    // beyond the bound — but was never enrolled.
    assert!(!w.router().is_tracked(&w.agent));
    w.mint(&w.agent, FUNDING);
    w.router().deposit(&w.agent, &FLOAT_CAP);
    w.set_time(EXPIRES_AT + GRACE_WINDOW + 1);
    w.router().transfer(&w.agent, &w.victim, &FLOAT_CAP);

    assert_eq!(w.router().spent(&cert_id), 0);
    assert_eq!(w.router().post_expiry_spent(&cert_id).count, 0);

    for proof in [ProofType::BoundExceeded, ProofType::ExpiredCertificate] {
        let id = w.challenge(cert_id, proof, MIN_CHALLENGE_STAKE);
        w.resolve(id);
        assert!(w.cm().get_challenge(&id).verdict == CmVerdict::ChallengeFails);
    }

    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    assert_auditor_untouched(&w);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_router_fully_backed(&w);
}

// --- The arbiter path is unchanged -----------------------------------------

/// The same `BoundExceeded` breach, on the same certificate, settles two ways.
///
/// Through `resolve` it is hygiene: the counter is evidence, and evidence alone
/// never reaches the auditor's capital. Through `resolve_by_arbiter` with a
/// stated harm it slashes exactly like an arithmetic proof, because a human has
/// assessed what was actually lost.
///
/// That difference is the whole design. It is not an inconsistency to be tidied
/// up by making `resolve` slash too.
#[test]
fn the_arbiter_still_slashes_the_same_breach_when_it_states_harm() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);
    w.router().transfer(&w.agent, &w.victim, &11_0000000);
    assert!(w.router().spent(&cert_id) > SMALL_BOUND);

    // The arbiter assesses $1,200 of real harm: more than the $1,000 reserve,
    // so the residual draws on the allocation.
    let harm = 1_200_0000000i128;
    let id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &true, &harm);
    w.settle_window(cert_id);

    //   victim = min(1200, 1000) = $1,000  <- the operator's own reserve
    //   fee    = min(120, 0)     = $0      <- nothing left in that reserve
    //   slash  = 1200 - 1000     = $200    <- the treasury, within the $600 alloc
    assert_eq!(w.balance(&w.victim), RESERVE_CLAIM);
    assert_eq!(w.balance(&w.treasury), 200_0000000);
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(
        w.staking().get_stake(&w.auditor),
        AUDITOR_STAKE - 200_0000000
    );
    assert_eq!(w.free_stake(), ALLOCATION - 200_0000000);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
}

/// `FakeSignature` leaves no on-chain trace, so it is filed un-adjudicated and
/// waits for the arbiter. If the arbiter never rules, closing the window
/// returns the bond in full rather than forfeiting it: a claim nobody judged is
/// not a claim the challenger got wrong.
#[test]
fn fake_signature_still_needs_the_arbiter() {
    let (w, cert_id) = predicate_world(SMALL_BOUND);
    let before = w.balance(&w.challenger);
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    let ch = w.cm().get_challenge(&id);
    assert!(!ch.adjudicated);
    assert!(!ch.proven);
    assert!(ch.verdict == CmVerdict::Pending);

    w.settle_window(cert_id);

    assert!(w.verdict_of(id) == CmVerdict::Unadjudicated);
    assert_eq!(w.balance(&w.challenger), before);
    // The certificate survives — nothing was ever proven against it.
    assert!(!w.cm().is_settled(&cert_id));
    assert_auditor_untouched(&w);
}

/// The router is named once, by the arbiter, and can never be re-pointed. A
/// lying router could invalidate any certificate it liked, so the race that an
/// unauthenticated setter would create is closed the same way the treasury's is.
#[test]
fn the_router_is_set_once_and_only_by_the_arbiter() {
    let (w, _) = predicate_world(SMALL_BOUND);
    assert_eq!(w.cm().get_router(), w.router);

    // Already set — even the arbiter cannot re-point it.
    assert!(w.cm().try_set_router(&w.challenger).is_err());
    assert_eq!(w.cm().get_router(), w.router);

    // And on a fresh, unwired ChallengeManager, a stranger cannot claim it.
    let fresh = w.env.register(ChallengeManager, ());
    let cm = ChallengeManagerClient::new(&w.env, &fresh);
    cm.initialize(
        &w.registry,
        &w.staking,
        &w.vault,
        &w.escrow,
        &w.token,
        &w.arbiter,
        &w.treasury,
        &MIN_CHALLENGE_STAKE,
    );
    w.env.set_auths(&[]);
    assert!(cm.try_set_router(&w.router).is_err());
    w.mock_all_auths();
    cm.set_router(&w.router);
    assert_eq!(cm.get_router(), w.router);
}

// ---------------------------------------------------------------------------
// 10. The premium economy — PremiumVault and step 4 of the waterfall
// ---------------------------------------------------------------------------
//
// The operator buys coverage priced on `bound × duration`; it accrues to the
// auditor straight-line as yield on the capital they allocated; a configurable
// share is the protocol's; and a slashed auditor forfeits what they have not
// already withdrawn.

/// A $1,500 bound covered for exactly one year, so the arithmetic divides
/// evenly and every assertion below can be an exact stroop figure.
const PREMIUM_BOUND: i128 = 1_500_0000000;
const YEAR: u64 = SECONDS_PER_YEAR;
/// 2% of $1,500 for one year.
const PREMIUM: i128 = 30_0000000;
/// 10% of the premium, taken at payment time.
const PREMIUM_FEE: i128 = 3_0000000;
/// What is left for the auditor to accrue.
const PREMIUM_POT: i128 = PREMIUM - PREMIUM_FEE; // $27

/// Staked, published, funded, attested, and covered — a certificate whose
/// coverage runs from t=1,000 for a full year.
fn premium_world(reserve_deposit: i128) -> (BoundWorld, u64) {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(PREMIUM_BOUND, RESERVE_CLAIM, 1_000 + YEAR);
    // DESIGN-V2 §4: fully funded to get attested, then drawn back down to the
    // deposit this scenario wants behind it.
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, reserve_deposit);
    w.pay_premium(cert_id);
    (w, cert_id)
}

/// **The pricing sanity check, end to end through the Registry.**
///
/// A $1,500 bound covered for 90 days at 200 bps:
///
///   15_000_000_000 * 200 * 7_776_000 / (10_000 * 31_536_000) = 73_972_602
///
/// The exact rational value is 73_972_602.7397…; integer division truncates it
/// down, so the operator is charged no more than the exact price. Asserted as
/// the exact integer, and confirmed against the operator's real balance.
#[test]
fn a_ninety_day_certificate_prices_at_exactly_73_972_602_stroops() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let ninety_days = 90 * 24 * 60 * 60;
    assert_eq!(ninety_days, 7_776_000u64);
    let cert_id = w.publish_cert(PREMIUM_BOUND, RESERVE_CLAIM, 1_000 + ninety_days);
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, 0);

    // Quoted before it is bought…
    assert_eq!(w.premium().quote_cert(&cert_id), 73_972_602);
    assert_eq!(w.premium().quote(&PREMIUM_BOUND, &ninety_days), 73_972_602);

    // …and charged exactly, in real money.
    let before = w.balance(&w.operator);
    w.pay_premium(cert_id);
    assert_eq!(w.premium_of(cert_id), 73_972_602);
    assert_eq!(before - w.balance(&w.operator), 73_972_602);

    // Linear in bound and in duration at magnitudes where the division is
    // exact: a full year at 200 bps is exactly 2% of the bound.
    assert_eq!(w.premium().quote(&PREMIUM_BOUND, &YEAR), PREMIUM);
    assert_eq!(w.premium().quote(&(PREMIUM_BOUND * 2), &YEAR), PREMIUM * 2);
    assert_eq!(w.premium().quote(&PREMIUM_BOUND, &(YEAR / 2)), PREMIUM / 2);

    // A zero bound or a zero duration costs zero and does not panic.
    assert_eq!(w.premium().quote(&0i128, &YEAR), 0);
    assert_eq!(w.premium().quote(&PREMIUM_BOUND, &0u64), 0);

    // A hostile bound errors rather than wrapping. `overflow-checks = true` is
    // on, and the contract additionally uses `checked_mul` so this is a named
    // failure and not a profile setting.
    assert!(w.premium().try_quote(&i128::MAX, &YEAR).is_err());
}

/// Straight-line accrual at 0%, 50% and 100%, the protocol's share sitting in
/// the treasury and out of the auditor's reach, and accrual capped at the pot
/// however far past expiry the clock runs.
#[test]
fn premium_accrues_straight_line_and_the_protocol_share_is_never_claimable() {
    let (w, cert_id) = premium_world(RESERVE_CLAIM);

    // The fee left for the treasury the moment the premium was paid.
    assert_eq!(w.premium_of(cert_id), PREMIUM);
    assert_eq!(w.balance(&w.treasury), PREMIUM_FEE);
    assert_eq!(w.balance(&w.premium_vault), PREMIUM_POT);
    assert_eq!(w.balance(&w.operator), FUNDING - RESERVE_CLAIM - PREMIUM);

    // 0% — nothing accrued yet.
    assert_eq!(w.accrued(cert_id), 0);
    assert_eq!(w.premium().claimable(&cert_id), 0);

    // 50% — exactly half the pot.
    w.set_time(1_000 + YEAR / 2);
    assert_eq!(w.accrued(cert_id), PREMIUM_POT / 2);
    assert_eq!(w.accrued(cert_id), 13_5000000);

    // 100% — the whole pot, and not a stroop of the protocol's share.
    w.set_time(1_000 + YEAR);
    assert_eq!(w.accrued(cert_id), PREMIUM_POT);

    // Accrual never exceeds the premium, however late it is read.
    w.set_time(1_000 + YEAR * 100);
    assert_eq!(w.accrued(cert_id), PREMIUM_POT);

    // The auditor withdraws it all, and the treasury's share is still there.
    assert_eq!(w.claim_premium(cert_id), PREMIUM_POT);
    assert_eq!(w.balance(&w.auditor), FUNDING - AUDITOR_STAKE + PREMIUM_POT);
    assert_eq!(w.balance(&w.premium_vault), 0);
    assert_eq!(w.balance(&w.treasury), PREMIUM_FEE);

    // Claiming again pays nothing.
    assert_eq!(w.claim_premium(cert_id), 0);
    assert_eq!(w.balance(&w.auditor), FUNDING - AUDITOR_STAKE + PREMIUM_POT);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// Claim, let more accrue, claim again: the running total is right and the
/// vault is never over-drawn.
#[test]
fn claim_accrue_claim_pays_the_right_total_and_never_twice() {
    let (w, cert_id) = premium_world(RESERVE_CLAIM);
    let auditor_start = FUNDING - AUDITOR_STAKE;

    w.set_time(1_000 + YEAR / 4);
    assert_eq!(w.claim_premium(cert_id), PREMIUM_POT / 4);
    assert_eq!(w.balance(&w.auditor), auditor_start + PREMIUM_POT / 4);
    // Immediately again — nothing new has accrued.
    assert_eq!(w.claim_premium(cert_id), 0);
    assert_eq!(w.balance(&w.auditor), auditor_start + PREMIUM_POT / 4);

    w.set_time(1_000 + YEAR / 2);
    assert_eq!(w.claim_premium(cert_id), PREMIUM_POT / 4);
    assert_eq!(w.balance(&w.auditor), auditor_start + PREMIUM_POT / 2);

    w.set_time(1_000 + YEAR);
    assert_eq!(w.claim_premium(cert_id), PREMIUM_POT / 2);
    assert_eq!(w.balance(&w.auditor), auditor_start + PREMIUM_POT);
    assert_eq!(w.balance(&w.premium_vault), 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **The forfeiture rule, both halves, in real balances — and step 4 of the
/// waterfall doing what it says.**
///
/// The auditor claims a quarter of their yield, then the certificate is proven
/// under-funded and they are slashed. They keep the quarter. The accrued-but-
/// unclaimed quarter goes to the victim as compensation — out of the operator's
/// own premium, which is what keeps rule 1 intact — and the unaccrued half goes
/// to the treasury.
///
///   claim $1,000, deposit $200 → proven harm $800
///   victim   = min(payable, reserve)          = $200   <- operator's reserve
///   fee      = 10% of harm, capped by what is left = $0
///   slash    = min(harm - victim, allocation) = $600   -> treasury
///   step 4   cap = harm - victim = $600, so the whole accrued-unclaimed
///            $6.75 reaches the victim; the unaccrued $13.50 -> treasury
#[test]
fn a_slashed_auditor_forfeits_unclaimed_premium_and_keeps_what_they_claimed() {
    let deposited = 200_0000000i128;
    let (w, cert_id) = premium_world(deposited);
    let auditor_start = FUNDING - AUDITOR_STAKE;

    // A quarter of the way through, the auditor takes their accrued yield.
    w.set_time(1_000 + YEAR / 4);
    let kept = PREMIUM_POT / 4; // $6.75
    assert_eq!(w.claim_premium(cert_id), kept);
    assert_eq!(w.balance(&w.auditor), auditor_start + kept);

    // The shortfall is challenged 72 hours before the halfway mark, so the
    // claim window closes — and settlement runs — exactly halfway through.
    w.set_time(1_000 + YEAR / 2 - CLAIM_WINDOW);
    let harm = RESERVE_CLAIM - deposited;
    assert_eq!(harm, 800_0000000);
    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);

    let accrued_unclaimed = PREMIUM_POT / 2 - kept; // $6.75
    let unaccrued = PREMIUM_POT / 2; // $13.50

    // HALF ONE OF THE RULE: what the auditor already claimed is theirs. There is
    // no clawback, and none is attempted.
    assert_eq!(w.balance(&w.auditor), auditor_start + kept);
    // HALF TWO: everything unclaimed is gone, permanently.
    assert_eq!(w.premium().claimable(&cert_id), 0);
    assert_eq!(w.claim_premium(cert_id), 0);
    w.set_time(1_000 + YEAR * 10);
    assert_eq!(w.claim_premium(cert_id), 0);
    assert_eq!(w.balance(&w.auditor), auditor_start + kept);

    // Step 4's split, exactly as specified.
    assert_eq!(w.balance(&w.victim), deposited + accrued_unclaimed);
    assert_eq!(w.balance(&w.treasury), ALLOCATION + PREMIUM_FEE + unaccrued);
    assert_eq!(w.balance(&w.premium_vault), 0);

    // The rest of the waterfall is untouched by step 4's arrival: the victim's
    // reserve compensation, the slash and the retirement are the same numbers
    // they would be with no premium at all.
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(
        w.staking().get_stake(&w.auditor),
        AUDITOR_STAKE - ALLOCATION
    );
    assert_eq!(w.balance(&w.challenger), FUNDING); // bond back, no fee left
    assert!(w.cm().get_challenge(&challenge_id).verdict == CmVerdict::ChallengeWins);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );

    // Conservation across every account, the premium vault included. Nothing
    // was created and nothing evaporated.
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// The victim's share of a forfeited premium is capped by the harm the
/// operator's own reserve did not already cover.
///
/// Here the reserve covers the harm in full, so the cap is zero and **none** of
/// the forfeited premium reaches the victim — all of it goes to the treasury.
/// Without the cap a large premium could pay a victim more than the harm proven
/// against the certificate, which the waterfall forbids.
#[test]
fn forfeited_premium_never_pays_a_victim_more_than_the_proven_harm() {
    let deposited = 900_0000000i128; // claim $1,000 → harm is only $100
    let (w, cert_id) = premium_world(deposited);

    w.set_time(1_000 + YEAR / 2);
    let harm = RESERVE_CLAIM - deposited;
    assert_eq!(harm, 100_0000000);
    let fee = harm * CHALLENGER_FEE_BPS / 10_000;

    let challenge_id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);

    // The reserve covered the harm, so victim_amount == harm and the step-4 cap
    // is exactly zero.
    assert_eq!(w.balance(&w.victim), harm);
    assert_eq!(w.balance(&w.challenger), FUNDING + fee);
    // Nothing was slashed either — and the auditor still forfeits, because the
    // certificate was proven harmful and they vouched for it.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.treasury), PREMIUM_FEE + PREMIUM_POT);
    assert_eq!(w.balance(&w.premium_vault), 0);
    assert_eq!(w.premium().claimable(&cert_id), 0);

    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **Hygiene mode.** The proof is true, nobody is evidenced as harmed and the
/// auditor is not slashed — so they keep and can still claim the yield accrued
/// up to the kill. Only the unaccrued remainder goes to the treasury.
///
/// It is not refunded to the operator, deliberately: both hygiene predicates are
/// manufacturable by the operator for the price of gas, and a refund would make
/// killing your own certificate free.
#[test]
fn hygiene_mode_freezes_accrual_and_sends_only_the_unaccrued_share_to_the_treasury() {
    let (w, cert_id) = premium_world(RESERVE_CLAIM);
    let auditor_start = FUNDING - AUDITOR_STAKE;

    // Drive the router's counter past the bound — a true, manufacturable proof.
    w.router().enroll(&w.agent, &cert_id, &FLOAT_CAP);
    w.mint(&w.operator, PREMIUM_BOUND * 2);
    w.router().deposit(&w.operator, &(PREMIUM_BOUND * 2));
    w.router()
        .transfer(&w.operator, &w.agent, &(PREMIUM_BOUND * 2));
    // 72 hours before the halfway mark, so the claim window closes exactly
    // halfway through the term and the accrual arithmetic below stays clean.
    w.set_time(1_000 + YEAR / 2 - CLAIM_WINDOW);
    w.router()
        .transfer(&w.agent, &w.victim, &(PREMIUM_BOUND + 1));
    assert!(w.router().spent(&cert_id) > PREMIUM_BOUND);

    let treasury_before = w.balance(&w.treasury);
    let challenge_id = w.challenge(cert_id, ProofType::BoundExceeded, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);

    // Hygiene: certificate dead, reserve untouched, allocation retired whole.
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);

    // The premium: the unaccrued half to the treasury…
    let unaccrued = PREMIUM_POT / 2;
    assert_eq!(w.balance(&w.treasury), treasury_before + unaccrued);
    // …and the accrued half still the auditor's, still claimable.
    assert_eq!(w.premium().claimable(&cert_id), PREMIUM_POT / 2);
    assert_eq!(w.claim_premium(cert_id), PREMIUM_POT / 2);
    assert_eq!(w.balance(&w.auditor), auditor_start + PREMIUM_POT / 2);
    assert_eq!(w.balance(&w.premium_vault), 0);

    // Accrual really is frozen: the clock running on adds nothing.
    w.set_time(1_000 + YEAR * 5);
    assert_eq!(w.accrued(cert_id), PREMIUM_POT / 2);
    assert_eq!(w.claim_premium(cert_id), 0);
}

/// Two certificates in one vault. Premium, accrual, claiming and forfeiture on
/// one must not touch the other by a single stroop.
#[test]
fn premium_and_accrual_on_one_certificate_do_not_touch_another() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE * 2);

    let cert_a = w.publish_cert(PREMIUM_BOUND, RESERVE_CLAIM, 1_000 + YEAR);
    let cert_b = w.publish_cert_for(
        &w.operator2,
        &w.agent2,
        PREMIUM_BOUND * 2, // twice the bound → twice the premium
        RESERVE_CLAIM,
        1_000 + YEAR,
    );
    w.attest_with_reserve_and(cert_a, RESERVE_CLAIM, 200_0000000, ALLOCATION);
    w.deposit_reserve(cert_b, RESERVE_CLAIM);
    w.attest_with(cert_b, ALLOCATION);
    w.pay_premium(cert_a);
    w.pay_premium(cert_b);

    assert_eq!(w.premium_of(cert_a), PREMIUM);
    assert_eq!(w.premium_of(cert_b), PREMIUM * 2);
    assert_eq!(w.balance(&w.premium_vault), PREMIUM_POT * 3);
    assert_eq!(
        w.balance(&w.operator2),
        FUNDING - RESERVE_CLAIM - PREMIUM * 2
    );

    // Accrual is independent.
    w.set_time(1_000 + YEAR / 4);
    assert_eq!(w.accrued(cert_a), PREMIUM_POT / 4);
    assert_eq!(w.accrued(cert_b), PREMIUM_POT / 2);

    // A's auditor is slashed and A's premium is forfeited entirely. The claim
    // is filed 72 hours before the halfway mark so its window closes there.
    w.set_time(1_000 + YEAR / 2 - CLAIM_WINDOW);
    let challenge_id = w.challenge(cert_a, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.resolve(challenge_id);
    assert_eq!(w.premium().claimable(&cert_a), 0);

    // B is untouched: same pot, same accrual, still fully claimable.
    assert_eq!(w.premium().get_coverage(&cert_b).yield_pot, PREMIUM_POT * 2);
    assert_eq!(w.premium().claimable(&cert_b), PREMIUM_POT);
    assert_eq!(w.balance(&w.premium_vault), PREMIUM_POT * 2);
    assert_eq!(w.reserve_of(cert_b), RESERVE_CLAIM);

    // And B pays out in full at the end of its own term.
    w.set_time(1_000 + YEAR);
    assert_eq!(w.claim_premium(cert_b), PREMIUM_POT * 2);
    assert_eq!(w.balance(&w.premium_vault), 0);

    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// An operator cannot buy coverage twice, and cannot buy it for a certificate
/// nobody has vouched for — the premium is yield on *staked* capital, so there
/// has to be an allocation behind it first.
#[test]
fn coverage_is_bought_once_and_only_for_an_attested_certificate() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(PREMIUM_BOUND, RESERVE_CLAIM, 1_000 + YEAR);

    // Pending, not attested: no auditor to accrue to.
    assert!(w.premium().try_pay_premium(&cert_id).is_err());
    assert!(!w.premium().is_paid(&cert_id));
    assert_eq!(w.balance(&w.operator), FUNDING);

    // DESIGN-V2 §4: attestation needs the reserve funded, and the operator gets
    // it straight back out, so their balance below is still purely the premium.
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, 0);
    w.pay_premium(cert_id);
    assert!(w.premium().try_pay_premium(&cert_id).is_err());
    assert_eq!(w.balance(&w.operator), FUNDING - PREMIUM);
}

/// The premium vault is named once, by the arbiter, and can never be re-pointed
/// — the same rule `set_router` follows, for the same reason: whoever names it
/// names the contract that is handed the forfeited premium and told where to
/// send it.
#[test]
fn the_premium_vault_is_set_once_and_only_by_the_arbiter() {
    let w = BoundWorld::new();
    assert!(w.cm().has_premium_vault());
    assert_eq!(w.cm().get_premium_vault(), w.premium_vault);

    // Already set — even the arbiter cannot re-point it.
    assert!(w.cm().try_set_premium_vault(&w.challenger).is_err());
    assert_eq!(w.cm().get_premium_vault(), w.premium_vault);

    // On a fresh, unwired ChallengeManager a stranger cannot claim it…
    let fresh = w.env.register(ChallengeManager, ());
    let cm = ChallengeManagerClient::new(&w.env, &fresh);
    cm.initialize(
        &w.registry,
        &w.staking,
        &w.vault,
        &w.escrow,
        &w.token,
        &w.arbiter,
        &w.treasury,
        &MIN_CHALLENGE_STAKE,
    );
    assert!(!cm.has_premium_vault());
    w.env.set_auths(&[]);
    assert!(cm.try_set_premium_vault(&w.premium_vault).is_err());
    w.mock_all_auths();
    cm.set_premium_vault(&w.premium_vault);
    assert_eq!(cm.get_premium_vault(), w.premium_vault);
}

/// Only the ChallengeManager can forfeit or terminate a coverage. A challenger,
/// a victim or the auditor themselves cannot reach either entry point — which is
/// what keeps step 4 a settlement step rather than a withdrawal anybody can aim.
#[test]
fn nobody_but_the_challenge_manager_can_forfeit_a_premium() {
    let (w, cert_id) = premium_world(RESERVE_CLAIM);
    w.set_time(1_000 + YEAR / 2);

    w.env.set_auths(&[]);
    assert!(w
        .premium()
        .try_forfeit(&cert_id, &w.challenger, &1_000_0000000i128)
        .is_err());
    assert!(w.premium().try_terminate(&cert_id).is_err());
    w.mock_all_auths();

    // The pot is exactly where it was.
    assert_eq!(w.balance(&w.premium_vault), PREMIUM_POT);
    assert_eq!(w.premium().claimable(&cert_id), PREMIUM_POT / 2);
}

// ---------------------------------------------------------------------------
// 12. DESIGN-V2 §1 — the claim window
//
// The defect: settlement invalidates the certificate, drains the reserve and
// retires the allocation, so under the old lifecycle the FIRST settled
// challenge foreclosed every honest claim behind it. A self-challenge for the
// minimum bond destroyed the coverage everyone else was relying on. The
// attacker gained nothing — the waterfall saw to that — and the victims still
// lost everything, which is just as fatal.
//
// The fix: a first valid challenge opens a 72-hour window instead of settling.
// Every claim filed into it is settled together, pro rata, once.
// ---------------------------------------------------------------------------

/// A certificate with a named reserve deposit, attested and funded, ready for
/// claims. `1_000` on the clock, `RESERVE_CLAIM` advertised.
fn window_world(deposit: i128) -> (BoundWorld, u64) {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    // DESIGN-V2 §4: attested against a full reserve, then drawn down to the
    // shortfall this scenario is about.
    w.attest_with_reserve(cert_id, RESERVE_CLAIM, deposit);
    (w, cert_id)
}

/// File an arbiter-assessed claim for a stated harm.
///
/// `FakeSignature` is the proof type whose harm is a **per-claimant
/// assessment** rather than a certificate-level number, which is what lets two
/// claimants in one window prove two different losses.
fn assessed_claim(
    w: &BoundWorld,
    cert_id: u64,
    challenger: &Address,
    victim: &Address,
    harm: i128,
) -> u64 {
    let id = w.challenge_as(
        challenger,
        victim,
        cert_id,
        ProofType::FakeSignature,
        MIN_CHALLENGE_STAKE,
    );
    w.cm().resolve_by_arbiter(&id, &true, &harm);
    id
}

/// **Two claimants, collateral enough for both → both paid in full, in one
/// settlement.**
///
/// This is the case the old lifecycle got most obviously wrong: the second
/// claimant's certificate was already dead by the time they filed, and the
/// reserve that would have covered them in full had gone to the first.
#[test]
fn two_claimants_with_enough_collateral_are_both_paid_in_full() {
    let (w, cert_id) = window_world(RESERVE_CLAIM); // $1,000 of real reserve

    let harm_a = 200_0000000i128;
    let harm_b = 300_0000000i128;
    let a = assessed_claim(&w, cert_id, &w.challenger, &w.victim, harm_a);
    let b = assessed_claim(&w, cert_id, &w.challenger2, &w.victim2, harm_b);

    // One window, two claims, and nothing has moved yet.
    assert_eq!(w.cm().get_window(&cert_id).unwrap().claims.len(), 2);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.victim2), 0);

    w.settle_window(cert_id);

    // Both upheld in the same settlement.
    assert!(w.verdict_of(a) == CmVerdict::ChallengeWins);
    assert!(w.verdict_of(b) == CmVerdict::ChallengeWins);

    // Paid in full — not pro rata, because $500 of harm fits inside $1,000 of
    // reserve with room to spare.
    assert_eq!(w.balance(&w.victim), harm_a);
    assert_eq!(w.balance(&w.victim2), harm_b);

    // Each challenger takes 10% of the harm THEY proved, plus their bond back.
    assert_eq!(w.balance(&w.challenger), FUNDING + harm_a / 10);
    assert_eq!(w.balance(&w.challenger2), FUNDING + harm_b / 10);

    // The operator's reserve covered everything, so the auditor is untouched.
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(
        w.reserve_of(cert_id),
        RESERVE_CLAIM - harm_a - harm_b - harm_a / 10 - harm_b / 10
    );
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Invalid
    );
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **Two claimants, not enough collateral → pro rata, and the collateral is
/// spent to the stroop.**
///
///   harm     = $600 + $400            = $1,000
///   collateral = reserve $100 + allocation $600 = $700
///   payable  = min(1,000, 700)        = $700
///   victims  = the whole $100 reserve, split 60/40 by proven harm
///   slash    = $600, the whole allocation, to the treasury
///
/// $700 of collateral goes out and $700 is accounted for. Nothing is stranded
/// in the vault and nothing is minted to cover the $300 of harm that no
/// collateral stands behind.
#[test]
fn two_claimants_short_of_collateral_are_paid_pro_rata_to_the_stroop() {
    let deposit = 100_0000000i128; // $100
    let (w, cert_id) = window_world(deposit);

    let harm_a = 600_0000000i128;
    let harm_b = 400_0000000i128;
    let a = assessed_claim(&w, cert_id, &w.challenger, &w.victim, harm_a);
    let b = assessed_claim(&w, cert_id, &w.challenger2, &w.victim2, harm_b);
    w.settle_window(cert_id);

    assert!(w.verdict_of(a) == CmVerdict::ChallengeWins);
    assert!(w.verdict_of(b) == CmVerdict::ChallengeWins);

    // 60/40 of the reserve, exactly in proportion to proven harm.
    let paid_a = 60_0000000i128;
    let paid_b = 40_0000000i128;
    assert_eq!(w.balance(&w.victim), paid_a);
    assert_eq!(w.balance(&w.victim2), paid_b);

    // NO DUST STRANDED: the reserve is empty, and what left it is exactly what
    // the victims received.
    assert_eq!(paid_a + paid_b, deposit);
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.balance(&w.vault), 0);

    // NONE CONJURED: total outflow is the collateral and not a stroop more,
    // even though $1,000 of harm was proven against it.
    let slash = ALLOCATION;
    assert_eq!(w.balance(&w.treasury), slash);
    assert_eq!(paid_a + paid_b + slash, deposit + ALLOCATION);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE - slash);

    // The fee is a share of the reserve, and the reserve was exhausted by the
    // victims first — so both challengers get their bond back and no more.
    assert_eq!(w.balance(&w.challenger), FUNDING);
    assert_eq!(w.balance(&w.challenger2), FUNDING);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **The rounding remainder goes to the treasury, and it is the only place it
/// could go.**
///
/// $1 of reserve split between claims of $1 and $2 of harm is 3333333 and
/// 6666666 stroops: one stroop short. Handing the odd stroop to "the largest
/// claim" or "the first claim" would make a payout depend on a tie-break, and a
/// tie-break is ordering value — the exact thing this window exists to destroy.
/// The treasury is not a party to the window and cannot be gamed into being
/// one.
#[test]
fn the_pro_rata_remainder_goes_to_the_treasury_and_never_goes_missing() {
    let deposit = 1_0000000i128; // $1
    let (w, cert_id) = window_world(deposit);

    assessed_claim(&w, cert_id, &w.challenger, &w.victim, 1_0000000);
    assessed_claim(&w, cert_id, &w.challenger2, &w.victim2, 2_0000000);
    w.settle_window(cert_id);

    let paid_a = 3_333_333i128;
    let paid_b = 6_666_666i128;
    assert_eq!(w.balance(&w.victim), paid_a);
    assert_eq!(w.balance(&w.victim2), paid_b);
    assert_eq!(paid_a + paid_b, deposit - 1);

    // The missing stroop is in the treasury, alongside the $2 slash — not left
    // behind in the vault, and not created out of nothing.
    let slash = 2_0000000i128;
    assert_eq!(w.balance(&w.treasury), slash + 1);
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.balance(&w.vault), 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **THE HEADLINE. A minimum-bond self-challenge cannot foreclose an honest
/// claim filed behind it.**
///
/// The operator underfunds their own certificate, then self-challenges for the
/// minimum bond naming their own address as the victim — the attack §1 exists
/// to kill. Under the old lifecycle that single settlement invalidated the
/// certificate, drained the reserve and retired the allocation before any
/// honest victim could file: their recovery was **zero**.
///
/// Now the self-challenge merely opens the window it was trying to slam shut.
/// The honest claimant files into the same window and is paid out of the same
/// reserve, and the ONLY effect the self-challenge has on them is its own
/// pro-rata dilution — one extra claim against a certificate-level shortfall,
/// so half instead of all. Not zero.
///
/// The self-dealer still gains nothing: their "compensation" is their own
/// reserve moving from their left pocket to their right, and the auditor's
/// allocation goes to a treasury they do not control.
#[test]
fn a_self_challenge_cannot_foreclose_an_honest_claim_in_the_same_window() {
    let deposit = 200_0000000i128; // $200 against a $1,000 claim
    let shortfall = RESERVE_CLAIM - deposit; // $800

    // --- the attack: self-challenge first, honest claim second -------------
    let (w, cert_id) = window_world(deposit);
    let operator_after_funding = w.balance(&w.operator);

    // The operator files first, for the minimum bond, naming themselves.
    let selfish = w.challenge_as(
        &w.operator,
        &w.operator,
        cert_id,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    // It did not settle. It opened a window.
    assert!(w.verdict_of(selfish) == CmVerdict::Pending);
    assert!(w.cm().get_window(&cert_id).is_some());
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );

    // An honest victim, who had no way of front-running anybody, files second.
    let honest = w.challenge_as(
        &w.challenger2,
        &w.victim2,
        cert_id,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    w.settle_window(cert_id);

    assert!(w.verdict_of(selfish) == CmVerdict::ChallengeWins);
    assert!(w.verdict_of(honest) == CmVerdict::ChallengeWins);

    // The $800 shortfall is a property of the CERTIFICATE, so the two claims
    // share it — $400 of weight each — and the $200 reserve splits evenly.
    let honest_share = deposit / 2;
    assert_eq!(w.balance(&w.victim2), honest_share);
    assert_eq!(honest_share, 100_0000000);

    // The self-dealer is exactly where they started, less the reserve they
    // burned: their bond came back and their "compensation" was their own money.
    assert_eq!(w.balance(&w.operator), operator_after_funding + deposit / 2);
    // And the auditor's allocation went to the treasury, which the operator
    // does not control. Manufacturing the proof paid them nothing.
    assert_eq!(w.balance(&w.treasury), ALLOCATION);

    // --- the control: the honest claim alone -------------------------------
    let (c, cert_c) = window_world(deposit);
    let alone = c.challenge_as(
        &c.challenger2,
        &c.victim2,
        cert_c,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    c.settle_window(cert_c);
    assert!(c.verdict_of(alone) == CmVerdict::ChallengeWins);
    let undiluted = c.balance(&c.victim2);
    assert_eq!(undiluted, deposit);

    // THE PROPERTY: the honest claimant's recovery fell by exactly its own
    // pro-rata dilution — one of two claims instead of one of one — and by
    // nothing else. Under first-resolver-takes-all it would have been zero.
    assert_eq!(honest_share * 2, undiluted);
    assert!(honest_share > 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
    assert_eq!(shortfall, 800_0000000);
}

/// **Ordering inside a window is worth nothing.** The same two claims filed in
/// the opposite order produce identical payouts, to the stroop, everywhere.
///
/// If this ever fails, the race has been rebuilt somewhere rather than removed.
#[test]
fn filing_order_inside_a_window_changes_no_payout() {
    let deposit = 100_0000000i128;
    let harm_a = 600_0000000i128;
    let harm_b = 400_0000000i128;

    let (first, cert_1) = window_world(deposit);
    assessed_claim(&first, cert_1, &first.challenger, &first.victim, harm_a);
    assessed_claim(&first, cert_1, &first.challenger2, &first.victim2, harm_b);
    first.settle_window(cert_1);

    // Same world, same claims, filed the other way round.
    let (second, cert_2) = window_world(deposit);
    assessed_claim(
        &second,
        cert_2,
        &second.challenger2,
        &second.victim2,
        harm_b,
    );
    assessed_claim(&second, cert_2, &second.challenger, &second.victim, harm_a);
    second.settle_window(cert_2);

    for (a, b) in [
        (first.balance(&first.victim), second.balance(&second.victim)),
        (
            first.balance(&first.victim2),
            second.balance(&second.victim2),
        ),
        (
            first.balance(&first.challenger),
            second.balance(&second.challenger),
        ),
        (
            first.balance(&first.challenger2),
            second.balance(&second.challenger2),
        ),
        (
            first.balance(&first.treasury),
            second.balance(&second.treasury),
        ),
        (first.reserve_of(cert_1), second.reserve_of(cert_2)),
        (
            first.staking().get_stake(&first.auditor),
            second.staking().get_stake(&second.auditor),
        ),
    ] {
        assert_eq!(a, b);
    }
    // Sanity: the payouts being compared are not all trivially zero.
    assert!(first.balance(&first.victim) > 0);
    assert!(first.balance(&first.victim2) > 0);
}

/// A claim filed one ledger after the window's close is refused. The window has
/// to be closed and settled before anything else happens to the certificate —
/// otherwise "settlement runs once, over all admitted claims" is not true.
#[test]
fn a_claim_filed_after_the_window_closes_is_rejected() {
    let (w, cert_id) = window_world(200_0000000);
    w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    let closes_at = w.cm().window_closes_at(&cert_id);

    // One ledger before the close: still admitted.
    w.set_time(closes_at - 1);
    let inside = w.challenge_as(
        &w.challenger2,
        &w.victim2,
        cert_id,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    assert_eq!(w.cm().get_window(&cert_id).unwrap().claims.len(), 2);

    // One ledger after: refused, and the challenger's money never moves.
    w.set_time(closes_at + 1);
    let before = w.balance(&w.challenger2);
    assert!(w
        .cm()
        .try_challenge(
            &w.challenger2,
            &cert_id,
            &ProofType::InsufficientReserve,
            &w.victim2,
            &MIN_CHALLENGE_STAKE,
        )
        .is_err());
    assert_eq!(w.balance(&w.challenger2), before);
    assert_eq!(w.cm().get_window(&cert_id).unwrap().claims.len(), 2);

    // And once it is settled, no further claim may ever open a second window.
    w.cm().close_window(&cert_id);
    assert!(w.verdict_of(inside) == CmVerdict::ChallengeWins);
    assert!(w.cm().is_settled(&cert_id));
    assert!(w
        .cm()
        .try_challenge(
            &w.challenger2,
            &cert_id,
            &ProofType::InsufficientReserve,
            &w.victim2,
            &MIN_CHALLENGE_STAKE,
        )
        .is_err());
}

/// **The certificate is frozen while a window is open**: the operator cannot
/// withdraw the reserve and the auditor cannot free the allocation, even after
/// the ordinary settlement deadline has passed.
///
/// The clock is deliberately set to `SETTLEMENT_DEADLINE` before the claim is
/// filed. At that instant `expires_at + CHALLENGE_WINDOW` has already elapsed,
/// so the only thing still holding the collateral is the claim window — which
/// is exactly what "expiry must not let anything escape" means.
#[test]
fn an_open_window_freezes_the_reserve_and_the_allocation_past_expiry() {
    let (w, cert_id) = window_world(900_0000000); // $100 short of its claim

    w.set_time(SETTLEMENT_DEADLINE);
    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);

    // The Registry's settlement deadline is now the window's close, not expiry.
    let closes_at = w.cm().window_closes_at(&cert_id);
    assert_eq!(closes_at, SETTLEMENT_DEADLINE + CLAIM_WINDOW);
    assert_eq!(
        w.registry().get_cert_settlement_deadline(&cert_id),
        closes_at
    );
    assert!(w.registry().is_frozen(&cert_id));

    // Neither side of the collateral can walk away.
    assert!(w.vault().try_release_to_operator(&cert_id).is_err());
    assert!(w.staking().try_release_allocation(&cert_id).is_err());
    assert_eq!(w.reserve_of(cert_id), 900_0000000);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);

    // Still frozen one ledger before the close.
    w.set_time(closes_at - 1);
    assert!(w.vault().try_release_to_operator(&cert_id).is_err());
    assert!(w.staking().try_release_allocation(&cert_id).is_err());

    // The operator cures the shortfall, so the window closes with nothing
    // admitted and the freeze lifts. Both sides unwind normally again.
    w.deposit_reserve(cert_id, 100_0000000);
    w.set_time(closes_at);
    w.cm().close_window(&cert_id);
    assert!(w.verdict_of(id) == CmVerdict::Cured);
    assert!(!w.registry().is_frozen(&cert_id));

    w.vault().release_to_operator(&cert_id);
    w.staking().release_allocation(&cert_id);
    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
}

/// A frozen certificate takes no new attestation. Without this an operator
/// under a live claim window could walk a fresh auditor onto the certificate
/// and put new collateral at risk for a breach that has already been filed.
#[test]
fn an_open_window_refuses_a_new_attestation() {
    let w = BoundWorld::new();
    w.set_time(1_000);
    w.stake_auditor(AUDITOR_STAKE);
    // Published, unfunded, and NOT yet attested: `InsufficientReserve` is true
    // against it the moment it exists.
    let cert_id = w.publish_cert(BOUND, RESERVE_CLAIM, EXPIRES_AT);
    w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    assert!(w.registry().is_frozen(&cert_id));

    assert!(w
        .registry()
        .try_attest(&w.auditor, &cert_id, &ALLOCATION)
        .is_err());
    assert_eq!(w.allocation_of(cert_id), 0);
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Pending
    );
}

// ---------------------------------------------------------------------------
// 13. DESIGN-V2 §2 — filing-time evaluation and `Cured`
//
// The window makes curing reachable: during it an operator could top the
// reserve back up, flip the predicate to false and pocket the challenger's
// forfeited bond. The protocol would be paying the operator for having been
// caught. Two mechanisms close it, and both are required.
// ---------------------------------------------------------------------------

/// **A cure returns the bond in full, and costs the challenger nothing.**
///
/// The operator remedies the shortfall inside the window. The certificate
/// survives, nobody is slashed, and the challenger — who was RIGHT at filing —
/// gets every stroop of their bond back.
#[test]
fn a_reserve_topped_up_during_the_window_resolves_as_cured() {
    let deposit = 900_0000000i128;
    let (w, cert_id) = window_world(deposit);
    let challenger_before = w.balance(&w.challenger);

    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    // The predicate was recorded true at filing, with its quantity.
    let ch = w.cm().get_challenge(&id);
    assert!(ch.proven);
    assert_eq!(ch.harm, RESERVE_CLAIM - deposit);

    // The operator fixes it, which is the behaviour we want to encourage.
    w.deposit_reserve(cert_id, RESERVE_CLAIM - deposit);
    w.settle_window(cert_id);

    assert!(w.verdict_of(id) == CmVerdict::Cured);
    // Bond back IN FULL. Not a fee, not a haircut, not a forfeiture.
    assert_eq!(w.balance(&w.challenger), challenger_before);
    assert_eq!(w.cm().get_bounty_pool(), 0);

    // The certificate survives and is still valid to a counterparty.
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    // (`verify` also weighs expiry, and 72 hours of window have carried the
    // clock past this certificate's short test expiry — the point here is that
    // the status is Verified rather than Invalid.)
    assert!(!w.cm().is_settled(&cert_id));

    // NO SLASH, and the auditor's stake is untouched to the stroop — allocation
    // included, because the certificate is still alive and still backed.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **A cure does not launder a claim that was wrong at filing.**
///
/// The first claim is right, so it opens the window. The operator then cures
/// it. A third party files afterwards — against a certificate that is, by then,
/// fully funded — and that claim is FALSE AT FILING. It is rejected and its
/// bond forfeited, exactly as §2 asks: a claim filed after a cure is rejected
/// rather than aggregated.
#[test]
fn a_claim_filed_after_the_cure_is_rejected_and_forfeits_its_bond() {
    let deposit = 900_0000000i128;
    let (w, cert_id) = window_world(deposit);

    let good = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.deposit_reserve(cert_id, RESERVE_CLAIM - deposit); // the cure

    let late = w.challenge_as(
        &w.challenger2,
        &w.victim2,
        cert_id,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    assert!(!w.cm().get_challenge(&late).proven); // false at filing, recorded so

    w.settle_window(cert_id);

    // The challenger who was right keeps their bond.
    assert!(w.verdict_of(good) == CmVerdict::Cured);
    assert_eq!(w.balance(&w.challenger), FUNDING);
    // The challenger who was wrong does not. This is the ONLY outcome that
    // forfeits a bond.
    assert!(w.verdict_of(late) == CmVerdict::ChallengeFails);
    assert_eq!(w.balance(&w.challenger2), FUNDING - MIN_CHALLENGE_STAKE);
    assert_eq!(w.cm().get_bounty_pool(), MIN_CHALLENGE_STAKE);

    // Certificate alive, auditor untouched.
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **The recorded state governs, not the live state.**
///
/// The claim is filed against an $800 shortfall. The operator then moves the
/// live number to $100 — enough to shrink the payout, not enough to cure. The
/// settlement pays the **$800 recorded at filing**, because what was true when
/// the challenge was filed stays true.
///
/// Without this the operator could always shave the claim down to a stroop
/// after being caught, which is the same theft as the bond grab wearing a
/// different hat.
#[test]
fn state_recorded_at_filing_governs_the_payout_not_live_state() {
    let deposit = 200_0000000i128;
    let (w, cert_id) = window_world(deposit);

    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    let recorded = w.cm().get_challenge(&id).harm;
    assert_eq!(recorded, 800_0000000);

    // The operator tops the reserve up to $900 — the live shortfall is now
    // $100, an eighth of what was filed against.
    w.deposit_reserve(cert_id, 700_0000000);
    assert_eq!(w.reserve_of(cert_id), 900_0000000);

    w.settle_window(cert_id);
    assert!(w.verdict_of(id) == CmVerdict::ChallengeWins);

    //   payable = min(800, 900 + 600) = $800   <- the RECORDED harm
    //   victim  = min(800, 900)       = $800
    //   fee     = min(80, 100)        = $80
    //   slash   = 800 - 800           = $0
    assert_eq!(w.balance(&w.victim), recorded);
    assert_eq!(w.balance(&w.challenger), FUNDING + recorded / 10);
    // Had live state governed, the victim would have received $100.
    assert!(w.balance(&w.victim) > 100_0000000);
    assert_eq!(
        w.reserve_of(cert_id),
        900_0000000 - recorded - recorded / 10
    );
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// A `Cured` resolution leaves the auditor's stake untouched — including when
/// the auditor has premium yield riding on the certificate. Nothing is slashed,
/// nothing is forfeited, and the coverage keeps accruing, because the
/// certificate never died.
#[test]
fn a_cured_resolution_leaves_the_auditors_stake_and_premium_alone() {
    let (w, cert_id) = premium_world(900_0000000);
    let auditor_start = FUNDING - AUDITOR_STAKE;

    let id = w.challenge(cert_id, ProofType::InsufficientReserve, MIN_CHALLENGE_STAKE);
    w.deposit_reserve(cert_id, RESERVE_CLAIM - 900_0000000);
    w.settle_window(cert_id);
    assert!(w.verdict_of(id) == CmVerdict::Cured);

    // Stake, allocation and free stake: all exactly as before.
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.balance(&w.staking), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.free_stake(), 0);
    assert_eq!(w.balance(&w.auditor), auditor_start);

    // The premium pot is intact and still accruing — no forfeit, no terminate.
    assert_eq!(w.premium().get_coverage(&cert_id).yield_pot, PREMIUM_POT);
    assert!(!w.premium().get_coverage(&cert_id).closed);
    assert_eq!(w.balance(&w.premium_vault), PREMIUM_POT);
    w.set_time(1_000 + YEAR);
    assert_eq!(w.claim_premium(cert_id), PREMIUM_POT);
    assert_eq!(w.balance(&w.auditor), auditor_start + PREMIUM_POT);

    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

// ---------------------------------------------------------------------------
// 13. DESIGN-V2 §10 — an arbiter rejection closes the window early
//
// The abuse: `FakeSignature` has no predicate, so it cannot be rejected at
// filing the way a false `InsufficientReserve` claim is. It opens a window and
// freezes the certificate for 72 hours — no attestation, no reserve
// withdrawal, no allocation release — for the price of the minimum bond. An
// arbiter ruling it false did nothing to shorten that.
//
// The fix: when the arbiter rejects a claim and NO OTHER LIVE CLAIM remains,
// the window closes on the spot through the same settlement path a natural
// close uses. The freeze lifts, the certificate survives, and the bond is
// forfeited exactly as it would have been at `closes_at` — getting rejected
// faster is not cheaper.
// ---------------------------------------------------------------------------

/// **The griefer's freeze is cut from 72 hours to one arbiter transaction.**
///
/// The bond is still forfeited, the certificate survives, and the auditor —
/// who was never accused of anything — is untouched.
#[test]
fn an_arbiter_rejection_closes_a_lone_claim_window_immediately() {
    let (w, cert_id) = window_world(RESERVE_CLAIM); // fully funded, nothing wrong

    let opened_at = 1_000u64;
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);

    // The freeze is real, and it runs 72 hours past the filing.
    let closes_at = w.cm().window_closes_at(&cert_id);
    assert_eq!(closes_at, opened_at + CLAIM_WINDOW);
    assert!(w.registry().is_frozen(&cert_id));
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);

    // The arbiter rules it false, then ends the window. Not one second of the
    // 72 hours has elapsed.
    w.cm().resolve_by_arbiter(&id, &false, &0i128);
    assert!(w.registry().is_frozen(&cert_id));
    w.cm().close_window_early(&id);
    assert_eq!(w.env.ledger().timestamp(), opened_at);

    // The window is gone and the freeze with it.
    assert!(w.cm().get_window(&cert_id).is_none());
    assert_eq!(w.cm().window_closes_at(&cert_id), 0);
    assert!(!w.registry().is_frozen(&cert_id));

    // The certificate SURVIVES. Nothing was settled, so a genuine claim can
    // still be filed against it later.
    assert!(!w.cm().is_settled(&cert_id));
    assert_eq!(
        w.registry().get_certificate(&cert_id).status,
        CertStatus::Verified
    );

    // THE BOND IS STILL FORFEITED. Speeding up the rejection did not make it
    // cheaper: the challenger is out the full minimum bond, exactly as they
    // would have been 72 hours later.
    assert!(w.verdict_of(id) == CmVerdict::ChallengeFails);
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_eq!(w.cm().get_bonds_held(), 0);
    assert_eq!(w.cm().get_bounty_pool(), MIN_CHALLENGE_STAKE);

    // Nobody else moved a stroop. The auditor in particular was never accused.
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.allocation_of(cert_id), ALLOCATION);
    assert_eq!(w.reserve_of(cert_id), RESERVE_CLAIM);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **The rail that matters: a genuine claimant's window is never cut short.**
///
/// A griefer's `FakeSignature` claim sits in the same window as a real,
/// predicate-backed `InsufficientReserve` claim. The arbiter rejects the
/// griefer — and the window must run its full 72 hours anyway, because the
/// honest claimant is standing in it and has not been paid.
#[test]
fn an_arbiter_rejection_cannot_cut_short_a_window_holding_a_live_claim() {
    let deposit = 900_0000000i128; // $100 short of the $1,000 claimed
    let (w, cert_id) = window_world(deposit);

    // The honest claim opens the window: a real, on-chain-provable shortfall.
    let honest = w.challenge_as(
        &w.challenger2,
        &w.victim2,
        cert_id,
        ProofType::InsufficientReserve,
        MIN_CHALLENGE_STAKE,
    );
    let closes_at = w.cm().window_closes_at(&cert_id);

    // The griefer files into it and is rejected by the arbiter.
    let grief = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&grief, &false, &0i128);

    // THE EARLY CLOSE IS REFUSED. A live claim remains.
    assert!(w.cm().try_close_window_early(&grief).is_err());
    assert!(w.cm().get_window(&cert_id).is_some());
    assert_eq!(w.cm().window_closes_at(&cert_id), closes_at);
    assert!(w.registry().is_frozen(&cert_id));

    // Still refused one ledger before the real close.
    w.set_time(closes_at - 1);
    assert!(w.cm().try_close_window_early(&grief).is_err());
    assert!(w.registry().is_frozen(&cert_id));

    // At the real close the honest claimant is paid IN FULL out of the
    // operator's own reserve, and the griefer's bond is forfeited.
    w.set_time(closes_at);
    w.cm().close_window(&cert_id);

    let harm = RESERVE_CLAIM - deposit; // the $100 shortfall
    assert!(w.verdict_of(honest) == CmVerdict::ChallengeWins);
    assert!(w.verdict_of(grief) == CmVerdict::ChallengeFails);
    assert_eq!(w.balance(&w.victim2), harm);
    assert_eq!(w.balance(&w.challenger2), FUNDING + harm / 10);
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);

    // The reserve covered it, so the auditor is untouched and the treasury empty.
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);
    assert_eq!(w.reserve_of(cert_id), deposit - harm - harm / 10);
    assert!(w.cm().is_settled(&cert_id));
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// An **un-adjudicated** claim is live too: the arbiter may still uphold it, so
/// it holds the window open exactly as a proven one does.
#[test]
fn an_unruled_claim_keeps_the_window_open() {
    let (w, cert_id) = window_world(RESERVE_CLAIM);

    let a = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    let b = w.challenge_as(
        &w.challenger2,
        &w.victim2,
        cert_id,
        ProofType::FakeSignature,
        MIN_CHALLENGE_STAKE,
    );

    // `a` is rejected; `b` has not been ruled on at all.
    w.cm().resolve_by_arbiter(&a, &false, &0i128);
    assert!(w.cm().try_close_window_early(&a).is_err());
    assert!(w.registry().is_frozen(&cert_id));

    // Once `b` is rejected as well, either rejected claim opens the door.
    w.cm().resolve_by_arbiter(&b, &false, &0i128);
    w.cm().close_window_early(&b);
    assert!(!w.registry().is_frozen(&cert_id));
    assert!(w.verdict_of(a) == CmVerdict::ChallengeFails);
    assert!(w.verdict_of(b) == CmVerdict::ChallengeFails);

    // Both bonds forfeited, neither returned.
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_eq!(w.balance(&w.challenger2), FUNDING - MIN_CHALLENGE_STAKE);
    assert_eq!(w.cm().get_bonds_held(), 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}

/// **Only the arbiter may end a window early**, and only against a claim the
/// arbiter actually ruled false.
#[test]
fn a_non_arbiter_cannot_close_a_window_early() {
    let (w, cert_id) = window_world(RESERVE_CLAIM);
    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &false, &0i128);

    // The operator has the most to gain from lifting their own freeze. Signing
    // as themselves is not enough.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.operator,
        invoke: &MockAuthInvoke {
            contract: &w.challenge_manager,
            fn_name: "close_window_early",
            args: (id,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.cm().try_close_window_early(&id).is_err());

    // Nor the challenger who filed it, hoping to shorten their own exposure.
    w.env.set_auths(&[]);
    w.env.mock_auths(&[MockAuth {
        address: &w.challenger,
        invoke: &MockAuthInvoke {
            contract: &w.challenge_manager,
            fn_name: "close_window_early",
            args: (id,).into_val(&w.env),
            sub_invokes: &[],
        },
    }]);
    assert!(w.cm().try_close_window_early(&id).is_err());

    // And with no signature at all.
    w.env.set_auths(&[]);
    assert!(w.cm().try_close_window_early(&id).is_err());

    w.mock_all_auths();
    assert!(w.registry().is_frozen(&cert_id));
    assert!(w.cm().get_window(&cert_id).is_some());

    // A claim the arbiter UPHELD is not a key to this door either.
    let (w2, cert2) = window_world(RESERVE_CLAIM);
    let upheld = w2.challenge(cert2, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w2.cm().resolve_by_arbiter(&upheld, &true, &10_0000000i128);
    assert!(w2.cm().try_close_window_early(&upheld).is_err());

    // Nor is a claim nobody has ruled on.
    let (w3, cert3) = window_world(RESERVE_CLAIM);
    let unruled = w3.challenge(cert3, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    assert!(w3.cm().try_close_window_early(&unruled).is_err());
    assert!(w3.registry().is_frozen(&cert3));
}

/// **After an early close the collateral unwinds on its normal schedule.**
///
/// The point of surviving the grief is that nothing about the certificate's
/// ordinary lifecycle changed: the operator gets the whole reserve back and the
/// auditor gets the whole allocation back, at `expires_at + CHALLENGE_WINDOW`
/// and not a second later.
#[test]
fn collateral_unwinds_normally_after_an_early_close() {
    let (w, cert_id) = window_world(RESERVE_CLAIM);
    let operator_before = w.balance(&w.operator);

    let id = w.challenge(cert_id, ProofType::FakeSignature, MIN_CHALLENGE_STAKE);
    w.cm().resolve_by_arbiter(&id, &false, &0i128);
    w.cm().close_window_early(&id);

    // The Registry's settlement deadline is back to the ordinary one — the
    // window's close is no longer extending it.
    assert_eq!(
        w.registry().get_cert_settlement_deadline(&cert_id),
        SETTLEMENT_DEADLINE
    );

    // Still locked until that deadline, exactly as an unchallenged certificate.
    assert!(w.vault().try_release_to_operator(&cert_id).is_err());
    assert!(w.staking().try_release_allocation(&cert_id).is_err());

    w.set_time(SETTLEMENT_DEADLINE);
    w.vault().release_to_operator(&cert_id);
    w.staking().release_allocation(&cert_id);

    assert_eq!(w.reserve_of(cert_id), 0);
    assert_eq!(w.balance(&w.operator), operator_before + RESERVE_CLAIM);
    assert_eq!(w.free_stake(), AUDITOR_STAKE);
    assert_eq!(w.staking().get_stake(&w.auditor), AUDITOR_STAKE);

    // CONSERVATION. The griefer's bond is the only money that moved, and it is
    // sitting in the ChallengeManager as forfeited surplus.
    assert_eq!(w.balance(&w.challenger), FUNDING - MIN_CHALLENGE_STAKE);
    assert_eq!(w.balance(&w.challenge_manager), MIN_CHALLENGE_STAKE);
    assert_eq!(w.cm().get_bonds_held(), 0);
    assert_eq!(w.balance(&w.victim), 0);
    assert_eq!(w.balance(&w.treasury), 0);
    assert_eq!(w.total_in_the_system(), FUNDING * 5);
}
