//! Unit tests for the PremiumVault.
//!
//! The vault reads five fields off the Registry and moves real tokens, so these
//! tests register a mock Registry and an in-memory Stellar Asset Contract. Every
//! assertion below is on a **balance** or on a stored amount — never on "it did
//! not panic".
//!
//! USDC amounts are written as `<dollars>_<7 decimals>`, e.g. `10_0000000` is
//! $10.
#![allow(clippy::inconsistent_digit_grouping)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Env,
};

// ---------------------------------------------------------------------------
// A mock Registry exposing exactly the five getters the vault reads.
// ---------------------------------------------------------------------------

mod mock_registry {
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    pub enum MockKey {
        Operator(u64),
        Auditor(u64),
        Bound(u64),
        IssuedAt(u64),
        ExpiresAt(u64),
        Verified(u64),
    }

    #[contract]
    pub struct MockRegistry;

    #[contractimpl]
    impl MockRegistry {
        #[allow(clippy::too_many_arguments)]
        pub fn set_cert(
            env: Env,
            cert_id: u64,
            operator: Address,
            auditor: Address,
            bound: i128,
            issued_at: u64,
            expires_at: u64,
            verified: bool,
        ) {
            let s = env.storage().persistent();
            s.set(&MockKey::Operator(cert_id), &operator);
            s.set(&MockKey::Auditor(cert_id), &auditor);
            s.set(&MockKey::Bound(cert_id), &bound);
            s.set(&MockKey::IssuedAt(cert_id), &issued_at);
            s.set(&MockKey::ExpiresAt(cert_id), &expires_at);
            s.set(&MockKey::Verified(cert_id), &verified);
        }

        pub fn get_cert_operator(env: Env, cert_id: u64) -> Address {
            env.storage()
                .persistent()
                .get(&MockKey::Operator(cert_id))
                .expect("certificate_not_found")
        }
        pub fn get_cert_auditor(env: Env, cert_id: u64) -> Address {
            env.storage()
                .persistent()
                .get(&MockKey::Auditor(cert_id))
                .expect("certificate_not_found")
        }
        pub fn get_cert_bound(env: Env, cert_id: u64) -> i128 {
            env.storage()
                .persistent()
                .get(&MockKey::Bound(cert_id))
                .expect("certificate_not_found")
        }
        pub fn get_cert_issued_at(env: Env, cert_id: u64) -> u64 {
            env.storage()
                .persistent()
                .get(&MockKey::IssuedAt(cert_id))
                .expect("certificate_not_found")
        }
        pub fn get_cert_expires_at(env: Env, cert_id: u64) -> u64 {
            env.storage()
                .persistent()
                .get(&MockKey::ExpiresAt(cert_id))
                .expect("certificate_not_found")
        }
        pub fn is_cert_verified(env: Env, cert_id: u64) -> bool {
            env.storage()
                .persistent()
                .get(&MockKey::Verified(cert_id))
                .expect("certificate_not_found")
        }
    }
}

use mock_registry::{MockRegistry, MockRegistryClient};

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// 200 bps — 2% of the bound per year.
const RATE_BPS: i128 = 200;
/// 10% of each premium is the protocol's.
const FEE_BPS: i128 = 1_000;
const FUNDING: i128 = 10_000_0000000;
const DAY: u64 = 86_400;

struct Fixture {
    env: Env,
    vault: Address,
    registry: Address,
    token: Address,
    operator: Address,
    auditor: Address,
    victim: Address,
    treasury: Address,
}

impl Fixture {
    fn new_with(rate_bps: i128, fee_bps: i128) -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let token_admin = Address::generate(&env);
        let operator = Address::generate(&env);
        let auditor = Address::generate(&env);
        let victim = Address::generate(&env);
        let treasury = Address::generate(&env);
        let cm = Address::generate(&env);

        let token = env
            .register_stellar_asset_contract_v2(token_admin.clone())
            .address();
        let registry_id = env.register(MockRegistry, ());
        let vault = env.register(PremiumVault, ());

        PremiumVaultClient::new(&env, &vault).initialize(
            &registry_id,
            &cm,
            &token,
            &treasury,
            &rate_bps,
            &fee_bps,
        );

        token::StellarAssetClient::new(&env, &token).mint(&operator, &FUNDING);

        Self {
            env,
            vault,
            registry: registry_id,
            token,
            operator,
            auditor,
            victim,
            treasury,
        }
    }

    fn new() -> Self {
        Self::new_with(RATE_BPS, FEE_BPS)
    }

    fn registry(&self) -> MockRegistryClient<'_> {
        MockRegistryClient::new(&self.env, &self.registry)
    }

    fn vault(&self) -> PremiumVaultClient<'_> {
        PremiumVaultClient::new(&self.env, &self.vault)
    }

    fn balance(&self, who: &Address) -> i128 {
        token::Client::new(&self.env, &self.token).balance(who)
    }

    fn set_time(&self, ts: u64) {
        self.env.ledger().set_timestamp(ts);
    }

    /// A verified certificate covering `[issued_at, issued_at + duration]`.
    fn cert(&self, cert_id: u64, bound: i128, issued_at: u64, duration: u64) {
        self.registry().set_cert(
            &cert_id,
            &self.operator,
            &self.auditor,
            &bound,
            &issued_at,
            &(issued_at + duration),
            &true,
        );
    }
}

// ---------------------------------------------------------------------------
// 1. Pricing
// ---------------------------------------------------------------------------

/// **The sanity check.** A $1,500 bound at 200 bps for 90 days.
///
///   15_000_000_000 * 200 * 7_776_000 / (10_000 * 31_536_000) = 73_972_602
///
/// The exact rational value is 73_972_602.7397…, so the truncated integer is
/// 73_972_602 stroops — $7.3972602. Asserted as the exact integer, not as a
/// rounded dollar figure.
#[test]
fn fifteen_hundred_dollars_at_200bps_for_90_days_is_73_972_602_stroops() {
    let f = Fixture::new();
    let bound = 1_500_0000000i128; // 15_000_000_000 stroops
    assert_eq!(bound, 15_000_000_000);
    let ninety_days = 90 * DAY;
    assert_eq!(ninety_days, 7_776_000);

    assert_eq!(f.vault().quote(&bound, &ninety_days), 73_972_602);

    // And the same number arrives through a certificate rather than a raw quote.
    f.cert(1, bound, 1_000, ninety_days);
    assert_eq!(f.vault().quote_cert(&1u64), 73_972_602);
}

/// Linear in both bound and duration.
///
/// Asserted at magnitudes where the division is **exact**, so the identity
/// really is an identity: with a coverage period that divides `SECONDS_PER_YEAR`
/// evenly, `premium = bound * rate / 10_000` scaled by the whole fraction of a
/// year, with no remainder to lose. A full year at 200 bps on a $1,500 bound is
/// exactly 2% of it, $30.
#[test]
fn premium_is_linear_in_bound_and_in_duration() {
    let f = Fixture::new();
    let year = SECONDS_PER_YEAR;
    let bound = 1_500_0000000i128;

    let base = f.vault().quote(&bound, &year);
    assert_eq!(base, 30_0000000); // exactly 2% of $1,500

    // Double the bound, double the premium.
    assert_eq!(f.vault().quote(&(bound * 2), &year), base * 2);
    assert_eq!(f.vault().quote(&(bound * 4), &year), base * 4);

    // Half the duration, half the premium. 31_536_000 / 2 and / 4 are whole
    // seconds, so these are exact too.
    assert_eq!(f.vault().quote(&bound, &(year / 2)), base / 2);
    assert_eq!(f.vault().quote(&bound, &(year / 4)), base / 4);

    // Both at once.
    assert_eq!(f.vault().quote(&(bound * 3), &(year / 4)), base * 3 / 4);
}

/// **Truncation, pinned where it actually bites.**
///
/// Linearity is exact above only because those inputs divide evenly. At 90 days
/// the exact price carries a `.7397…` remainder, and doubling either input does
/// **not** double the truncated integer — it gives one stroop more than twice
/// it, because two truncated halves lose more than one truncated whole.
///
/// This is asserted rather than avoided: the discrepancy is real, it is at most
/// one stroop, and a reader who later "fixes" the linearity test by rounding
/// should see this test fail.
#[test]
fn truncation_breaks_exact_linearity_by_at_most_one_stroop() {
    let f = Fixture::new();
    let bound = 1_500_0000000i128;
    let ninety = 90 * DAY;

    let base = f.vault().quote(&bound, &ninety);
    assert_eq!(base, 73_972_602);

    // Exact value 147_945_205.479… → 147_945_205, which is 2 * base + 1.
    let doubled_duration = f.vault().quote(&bound, &(ninety * 2));
    assert_eq!(doubled_duration, 147_945_205);
    assert_eq!(doubled_duration, base * 2 + 1);

    let doubled_bound = f.vault().quote(&(bound * 2), &ninety);
    assert_eq!(doubled_bound, 147_945_205);

    // The error is always downward and always sub-stroop: the truncated price
    // never exceeds the exact one.
    assert!(doubled_duration - base * 2 <= 1);
}

/// A zero bound or a zero duration costs zero, and nothing panics.
#[test]
fn zero_bound_or_zero_duration_costs_zero() {
    let f = Fixture::new();
    assert_eq!(f.vault().quote(&0i128, &(90 * DAY)), 0);
    assert_eq!(f.vault().quote(&1_500_0000000i128, &0u64), 0);
    assert_eq!(f.vault().quote(&0i128, &0u64), 0);
    // A negative bound is not a price either — it is nonsense, and it prices at
    // zero rather than at a negative premium the operator would be paid.
    assert_eq!(f.vault().quote(&-1_000i128, &(90 * DAY)), 0);
}

/// A zero-duration certificate can be "paid" for and moves no money at all.
#[test]
fn a_zero_duration_certificate_pays_nothing_and_does_not_panic() {
    let f = Fixture::new();
    f.set_time(1_000);
    f.cert(1, 1_500_0000000, 1_000, 0);

    f.vault().pay_premium(&1u64);

    assert_eq!(f.balance(&f.operator), FUNDING);
    assert_eq!(f.balance(&f.vault), 0);
    assert_eq!(f.balance(&f.treasury), 0);
    assert_eq!(f.vault().get_premium(&1u64), 0);
    assert!(f.vault().is_paid(&1u64));
    // Accrual on an empty pot is zero, not a division by zero.
    assert_eq!(f.vault().accrued(&1u64), 0);
    assert_eq!(f.vault().claimable(&1u64), 0);
}

/// **Overflow safety.** A hostile bound errors with a named contract failure
/// rather than wrapping into a small or negative premium.
#[test]
fn a_hostile_bound_errors_rather_than_wrapping() {
    let f = Fixture::new();
    // i128::MAX / 200 + 1: the first multiplication alone overflows.
    let hostile = i128::MAX / RATE_BPS + 1;
    assert!(f.vault().try_quote(&hostile, &(90 * DAY)).is_err());

    // A bound that survives the rate multiply but not the duration multiply.
    let hostile2 = i128::MAX / (RATE_BPS * (90 * DAY) as i128) + 1;
    assert!(f.vault().try_quote(&hostile2, &(90 * DAY)).is_err());

    // And a hostile duration, with an ordinary bound.
    assert!(f.vault().try_quote(&i128::MAX, &u64::MAX).is_err());

    // The honest case immediately below the wrap still prices fine, so the
    // guard is a ceiling and not a blanket refusal.
    assert!(f.vault().quote(&1_000_000_0000000i128, &(365 * DAY)) > 0);
}

#[test]
fn a_hostile_bound_errors_on_the_pay_path_too() {
    let f = Fixture::new();
    f.set_time(1_000);
    f.cert(1, i128::MAX, 1_000, 90 * DAY);
    assert!(f.vault().try_pay_premium(&1u64).is_err());
    // Nothing was recorded and nothing moved.
    assert!(!f.vault().is_paid(&1u64));
    assert_eq!(f.balance(&f.operator), FUNDING);
}

// ---------------------------------------------------------------------------
// 2. Payment and the protocol fee share
// ---------------------------------------------------------------------------

/// The premium leaves the operator, the fee share reaches the treasury
/// immediately, and only the remainder is ever claimable by the auditor.
#[test]
fn the_protocol_fee_reaches_the_treasury_and_is_not_claimable() {
    let f = Fixture::new();
    f.set_time(1_000);
    let bound = 1_500_0000000i128;
    let year = SECONDS_PER_YEAR;
    f.cert(1, bound, 1_000, year);

    let premium = 30_0000000i128; // exactly 2%
    let fee = premium * FEE_BPS / 10_000; // 10% → $3
    assert_eq!(fee, 3_0000000);
    let pot = premium - fee;

    f.vault().pay_premium(&1u64);

    assert_eq!(f.balance(&f.operator), FUNDING - premium);
    assert_eq!(f.balance(&f.treasury), fee);
    assert_eq!(f.balance(&f.vault), pot);

    let c = f.vault().get_coverage(&1u64);
    assert_eq!(c.premium, premium);
    assert_eq!(c.protocol_fee, fee);
    assert_eq!(c.yield_pot, pot);
    assert_eq!(c.premium, c.protocol_fee + c.yield_pot);

    // Even at the very end of coverage the auditor can never reach the fee.
    f.set_time(1_000 + year);
    assert_eq!(f.vault().accrued(&1u64), pot);
    f.vault().claim(&1u64);
    assert_eq!(f.balance(&f.auditor), pot);
    assert_eq!(f.balance(&f.vault), 0);
    assert_eq!(f.balance(&f.treasury), fee);
}

/// A zero fee share sends the whole premium to the auditor's pot.
#[test]
fn a_zero_fee_share_gives_the_auditor_the_whole_premium() {
    let f = Fixture::new_with(RATE_BPS, 0);
    f.set_time(1_000);
    f.cert(1, 1_500_0000000, 1_000, SECONDS_PER_YEAR);
    f.vault().pay_premium(&1u64);

    assert_eq!(f.balance(&f.treasury), 0);
    assert_eq!(f.balance(&f.vault), 30_0000000);
    assert_eq!(f.vault().get_coverage(&1u64).yield_pot, 30_0000000);
}

#[test]
#[should_panic(expected = "premium_already_paid")]
fn paying_twice_panics() {
    let f = Fixture::new();
    f.set_time(1_000);
    f.cert(1, 1_500_0000000, 1_000, SECONDS_PER_YEAR);
    f.vault().pay_premium(&1u64);
    f.vault().pay_premium(&1u64);
}

/// The premium is yield on **staked** capital, so an unattested certificate has
/// nothing to accrue to and cannot be covered.
#[test]
#[should_panic(expected = "cert_not_verified")]
fn an_unattested_certificate_cannot_buy_coverage() {
    let f = Fixture::new();
    f.set_time(1_000);
    f.registry().set_cert(
        &1u64,
        &f.operator,
        &f.auditor,
        &1_500_0000000i128,
        &1_000u64,
        &(1_000 + SECONDS_PER_YEAR),
        &false,
    );
    f.vault().pay_premium(&1u64);
}

#[test]
#[should_panic(expected = "invalid_fee")]
fn a_fee_share_above_one_hundred_percent_is_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let a = Address::generate(&env);
    let vault = env.register(PremiumVault, ());
    PremiumVaultClient::new(&env, &vault).initialize(&a, &a, &a, &a, &200i128, &10_001i128);
}

#[test]
#[should_panic(expected = "already_initialized")]
fn double_initialize_panics() {
    let f = Fixture::new();
    let a = Address::generate(&f.env);
    f.vault().initialize(&a, &a, &a, &a, &RATE_BPS, &FEE_BPS);
}

// ---------------------------------------------------------------------------
// 3. Straight-line accrual
// ---------------------------------------------------------------------------

/// Exact accrued amounts at 0%, 50% and 100% of the coverage period.
#[test]
fn accrual_is_straight_line_at_zero_half_and_full() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.vault().pay_premium(&1u64);

    let pot = 27_0000000i128; // $30 premium less the $3 fee
    assert_eq!(f.vault().get_coverage(&1u64).yield_pot, pot);

    // 0%
    assert_eq!(f.vault().accrued(&1u64), 0);
    assert_eq!(f.vault().claimable(&1u64), 0);

    // 25%
    f.set_time(start + year / 4);
    assert_eq!(f.vault().accrued(&1u64), pot / 4);

    // 50%
    f.set_time(start + year / 2);
    assert_eq!(f.vault().accrued(&1u64), pot / 2);
    assert_eq!(f.vault().accrued(&1u64), 13_5000000);

    // 100%
    f.set_time(start + year);
    assert_eq!(f.vault().accrued(&1u64), pot);
}

/// Accrual is capped at the pot however far past expiry the clock runs, and a
/// claim after that pays exactly the pot — not a stroop more.
#[test]
fn accrual_never_exceeds_the_premium_however_late_it_is_read() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.vault().pay_premium(&1u64);
    let pot = 27_0000000i128;

    for multiple in [1u64, 2, 10, 1_000] {
        f.set_time(start + year * multiple);
        assert_eq!(f.vault().accrued(&1u64), pot);
    }
    // A century later.
    f.set_time(start + year * 100);
    assert_eq!(f.vault().accrued(&1u64), pot);

    f.vault().claim(&1u64);
    assert_eq!(f.balance(&f.auditor), pot);
    assert_eq!(f.balance(&f.vault), 0);
}

/// A read before the coverage window opens accrues nothing.
#[test]
fn nothing_accrues_before_coverage_starts() {
    let f = Fixture::new();
    let start = 1_000_000u64;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, SECONDS_PER_YEAR);
    f.vault().pay_premium(&1u64);

    f.set_time(start - 500);
    assert_eq!(f.vault().accrued(&1u64), 0);
    assert_eq!(f.vault().claimable(&1u64), 0);
    assert_eq!(f.vault().claim(&1u64), 0);
    assert_eq!(f.balance(&f.auditor), 0);
}

// ---------------------------------------------------------------------------
// 4. Claiming
// ---------------------------------------------------------------------------

/// Claiming twice at the same instant pays once; claim–accrue–claim pays the
/// correct running total and never more than the pot.
#[test]
fn claiming_twice_does_not_pay_twice_and_the_totals_add_up() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.vault().pay_premium(&1u64);
    let pot = 27_0000000i128;

    // Halfway: claim half.
    f.set_time(start + year / 2);
    assert_eq!(f.vault().claim(&1u64), pot / 2);
    assert_eq!(f.balance(&f.auditor), pot / 2);

    // Immediately again: nothing new has accrued.
    assert_eq!(f.vault().claim(&1u64), 0);
    assert_eq!(f.balance(&f.auditor), pot / 2);
    assert_eq!(f.vault().get_claimed(&1u64), pot / 2);

    // Three-quarters: exactly the next quarter.
    f.set_time(start + year * 3 / 4);
    assert_eq!(f.vault().claim(&1u64), pot / 4);
    assert_eq!(f.balance(&f.auditor), pot * 3 / 4);

    // End: the last quarter, and the vault is empty.
    f.set_time(start + year);
    assert_eq!(f.vault().claim(&1u64), pot / 4);
    assert_eq!(f.balance(&f.auditor), pot);
    assert_eq!(f.balance(&f.vault), 0);

    // And a fourth claim after everything is drained pays nothing.
    assert_eq!(f.vault().claim(&1u64), 0);
    assert_eq!(f.balance(&f.auditor), pot);
}

#[test]
#[should_panic(expected = "no_coverage")]
fn claiming_an_uncovered_certificate_panics() {
    let f = Fixture::new();
    f.vault().claim(&7u64);
}

// ---------------------------------------------------------------------------
// 5. Forfeiture on a slash
// ---------------------------------------------------------------------------

/// **The rule, both halves, in real balances.**
///
/// The auditor claims a quarter of the coverage, then is slashed halfway
/// through. They keep the quarter they already took. The accrued-but-unclaimed
/// quarter goes to the victim (capped by harm), and the unaccrued half goes to
/// the treasury. Nothing is clawed back.
#[test]
fn a_slashed_auditor_forfeits_unclaimed_yield_but_keeps_what_they_claimed() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.vault().pay_premium(&1u64);
    let pot = 27_0000000i128;

    // A quarter in, the auditor claims.
    f.set_time(start + year / 4);
    f.vault().claim(&1u64);
    let kept = pot / 4;
    assert_eq!(f.balance(&f.auditor), kept);

    // Halfway, the ChallengeManager slashes with plenty of uncovered harm.
    f.set_time(start + year / 2);
    let accrued = pot / 2;
    let accrued_unclaimed = accrued - kept;
    let unaccrued = pot - accrued;

    let to_victim = f.vault().forfeit(&1u64, &f.victim, &1_000_0000000i128);
    assert_eq!(to_victim, accrued_unclaimed);

    // Half of the rule: they keep what they claimed. No clawback exists.
    assert_eq!(f.balance(&f.auditor), kept);
    // The other half: everything unclaimed is gone.
    assert_eq!(f.balance(&f.victim), accrued_unclaimed);
    assert_eq!(f.balance(&f.treasury), 3_0000000 + unaccrued); // fee + unaccrued
    assert_eq!(f.balance(&f.vault), 0);

    // And it stays gone: the clock moving on accrues nothing more.
    f.set_time(start + year * 10);
    assert_eq!(f.vault().claimable(&1u64), 0);
    assert_eq!(f.vault().claim(&1u64), 0);
    assert_eq!(f.balance(&f.auditor), kept);

    // Conservation: the operator's premium is now split across exactly three
    // places, and none of it evaporated.
    assert_eq!(
        f.balance(&f.operator)
            + f.balance(&f.auditor)
            + f.balance(&f.victim)
            + f.balance(&f.treasury)
            + f.balance(&f.vault),
        FUNDING
    );
}

/// The victim's share is capped by the harm the reserve did not cover. Anything
/// above the cap goes to the treasury, so the premium can never pay a victim
/// more than the harm proven against the certificate.
#[test]
fn the_victims_share_of_forfeited_premium_is_capped_by_harm() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.vault().pay_premium(&1u64);
    let pot = 27_0000000i128;

    // Fully accrued, nothing claimed: the whole pot is accrued-unclaimed.
    f.set_time(start + year);
    assert_eq!(f.vault().accrued(&1u64), pot);

    let cap = 1_0000000i128; // only $1 of harm left uncovered
    assert_eq!(f.vault().forfeit(&1u64, &f.victim, &cap), cap);
    assert_eq!(f.balance(&f.victim), cap);
    assert_eq!(f.balance(&f.treasury), 3_0000000 + pot - cap);
    assert_eq!(f.balance(&f.auditor), 0);
    assert_eq!(f.balance(&f.vault), 0);
}

/// A zero cap — the operator's own reserve already covered the harm in full —
/// sends the entire forfeited pot to the treasury and none of it to the victim.
#[test]
fn a_zero_harm_cap_sends_the_whole_forfeiture_to_the_treasury() {
    let f = Fixture::new();
    let start = 1_000u64;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, SECONDS_PER_YEAR);
    f.vault().pay_premium(&1u64);

    f.set_time(start + SECONDS_PER_YEAR / 2);
    assert_eq!(f.vault().forfeit(&1u64, &f.victim, &0i128), 0);
    assert_eq!(f.balance(&f.victim), 0);
    assert_eq!(f.balance(&f.treasury), 3_0000000 + 27_0000000);
    assert_eq!(f.balance(&f.vault), 0);
}

/// Forfeiting a certificate that never bought coverage is a silent no-op, so
/// the waterfall does not have to know whether a premium was ever paid.
#[test]
fn forfeiting_an_uncovered_certificate_moves_nothing() {
    let f = Fixture::new();
    assert_eq!(f.vault().forfeit(&9u64, &f.victim, &1_000_0000000i128), 0);
    assert_eq!(f.balance(&f.victim), 0);
    assert_eq!(f.balance(&f.treasury), 0);
}

/// Forfeiting twice pays out once. The second call finds a closed coverage.
#[test]
fn forfeiting_twice_pays_once() {
    let f = Fixture::new();
    let start = 1_000u64;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, SECONDS_PER_YEAR);
    f.vault().pay_premium(&1u64);
    f.set_time(start + SECONDS_PER_YEAR / 2);

    let first = f.vault().forfeit(&1u64, &f.victim, &1_000_0000000i128);
    let victim_after = f.balance(&f.victim);
    let treasury_after = f.balance(&f.treasury);
    assert!(first > 0);

    assert_eq!(f.vault().forfeit(&1u64, &f.victim, &1_000_0000000i128), 0);
    assert_eq!(f.balance(&f.victim), victim_after);
    assert_eq!(f.balance(&f.treasury), treasury_after);
}

#[test]
#[should_panic(expected = "invalid_cap")]
fn a_negative_harm_cap_is_rejected() {
    let f = Fixture::new();
    f.vault().forfeit(&1u64, &f.victim, &-1i128);
}

/// **Only the ChallengeManager can forfeit or terminate.** Anybody else — a
/// challenger, the victim, even the auditor — is refused.
#[test]
fn only_the_challenge_manager_may_forfeit_or_terminate() {
    let env = Env::default();
    // No blanket auth mocking: auth is the thing under test.
    let token_admin = Address::generate(&env);
    let operator = Address::generate(&env);
    let auditor = Address::generate(&env);
    let victim = Address::generate(&env);
    let treasury = Address::generate(&env);
    let cm = Address::generate(&env);

    let token = env
        .register_stellar_asset_contract_v2(token_admin)
        .address();
    let registry_id = env.register(MockRegistry, ());
    let vault = env.register(PremiumVault, ());
    let client = PremiumVaultClient::new(&env, &vault);
    client.initialize(&registry_id, &cm, &token, &treasury, &RATE_BPS, &FEE_BPS);

    MockRegistryClient::new(&env, &registry_id).set_cert(
        &1u64,
        &operator,
        &auditor,
        &1_500_0000000i128,
        &1_000u64,
        &(1_000 + SECONDS_PER_YEAR),
        &true,
    );

    // Nobody has authorized the ChallengeManager, so both settlement entry
    // points fail on `require_auth`.
    assert!(client.try_forfeit(&1u64, &victim, &0i128).is_err());
    assert!(client.try_terminate(&1u64).is_err());
}

// ---------------------------------------------------------------------------
// 6. Hygiene termination
// ---------------------------------------------------------------------------

/// Hygiene mode: the auditor was not slashed, so they keep the accrued share
/// and can still claim it. Only the unaccrued remainder goes to the treasury.
#[test]
fn hygiene_termination_leaves_the_accrued_share_claimable() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.vault().pay_premium(&1u64);
    let pot = 27_0000000i128;

    f.set_time(start + year / 2);
    let unaccrued = f.vault().terminate(&1u64);
    assert_eq!(unaccrued, pot / 2);
    assert_eq!(f.balance(&f.treasury), 3_0000000 + pot / 2);

    // The auditor's half is still theirs, and still claimable.
    assert_eq!(f.vault().claimable(&1u64), pot / 2);
    assert_eq!(f.vault().claim(&1u64), pot / 2);
    assert_eq!(f.balance(&f.auditor), pot / 2);
    assert_eq!(f.balance(&f.vault), 0);

    // Accrual is frozen: the clock running on adds nothing.
    f.set_time(start + year * 5);
    assert_eq!(f.vault().accrued(&1u64), pot / 2);
    assert_eq!(f.vault().claim(&1u64), 0);
}

#[test]
fn terminating_an_uncovered_certificate_moves_nothing() {
    let f = Fixture::new();
    assert_eq!(f.vault().terminate(&9u64), 0);
    assert_eq!(f.balance(&f.treasury), 0);
}

// ---------------------------------------------------------------------------
// 7. Per-certificate isolation
// ---------------------------------------------------------------------------

/// Two certificates in one vault. Paying, accruing, claiming and forfeiting one
/// does not move a stroop of the other.
#[test]
fn two_certificates_do_not_touch_each_other() {
    let f = Fixture::new();
    let start = 1_000u64;
    let year = SECONDS_PER_YEAR;
    f.set_time(start);
    f.cert(1, 1_500_0000000, start, year);
    f.cert(2, 3_000_0000000, start, year); // twice the bound

    f.vault().pay_premium(&1u64);
    f.vault().pay_premium(&2u64);

    let pot_a = 27_0000000i128;
    let pot_b = 54_0000000i128;
    assert_eq!(f.vault().get_coverage(&1u64).yield_pot, pot_a);
    assert_eq!(f.vault().get_coverage(&2u64).yield_pot, pot_b);
    assert_eq!(f.balance(&f.vault), pot_a + pot_b);

    // Accrual is independent.
    f.set_time(start + year / 2);
    assert_eq!(f.vault().accrued(&1u64), pot_a / 2);
    assert_eq!(f.vault().accrued(&2u64), pot_b / 2);

    // Forfeit A entirely. B is untouched.
    f.vault().forfeit(&1u64, &f.victim, &1_000_0000000i128);
    assert_eq!(f.vault().claimable(&1u64), 0);
    assert_eq!(f.vault().claimable(&2u64), pot_b / 2);
    assert_eq!(f.balance(&f.vault), pot_b);

    // B's auditor still gets every stroop of B, at the right time.
    f.set_time(start + year);
    assert_eq!(f.vault().claim(&2u64), pot_b);
    assert_eq!(f.balance(&f.vault), 0);

    // A is still dead.
    assert_eq!(f.vault().claim(&1u64), 0);
}
