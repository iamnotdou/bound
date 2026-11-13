//! Settlement waterfall, as a pure function so the math is unit tested apart
//! from storage.
//!
//! Ported 1:1 from `contracts/challenge-manager/src/lib.rs` `settle_fraud`
//! (Soroban). Steps 1-3 and 5 of that function; step 4 (forfeited premium)
//! does not exist here because the NEAR port pays its flat premium at attest
//! (see DESIGN.md). Every amount is in token base units (USDC, 6 decimals).

/// One admitted claim as the waterfall sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimIn {
    /// Harm this claim counts for. Arbitrated claims: the harm the arbiter
    /// assessed. `InsufficientReserve`: shortfall / number of such claims.
    /// Hygiene predicates: 0.
    pub weight: u128,
    /// True only for claims whose harm an arbiter assessed. Only these name a
    /// victim the protocol pays (Soroban R2).
    pub arbitrated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Payouts {
    /// min(total_harm, reserve + allocation)
    pub payable: u128,
    /// min(payable, reserve): the part of payable the operator's reserve covers.
    pub reserve_draw: u128,
    /// Per claim, paid to its victim (0 for non-arbitrated claims).
    pub victim_shares: Vec<u128>,
    /// reserve_draw minus what the victims received (predicate share and
    /// rounding), to the treasury.
    pub victim_remainder: u128,
    /// min(fee_bps of total_harm, reserve - reserve_draw)
    pub fee_pool: u128,
    /// Per claim, paid to its challenger, pro rata by weight.
    pub fee_shares: Vec<u128>,
    /// fee_pool minus what challengers received (rounding), to the treasury.
    pub fee_remainder: u128,
    /// min(payable - reserve_draw, allocation): the auditor's slash.
    pub slash: u128,
    /// Per claim, the part of the slash paid to its victim: unpaid
    /// arbitrated harm first (NEAR port; Soroban sent the whole slash to the
    /// treasury). Whatever the victims do not need goes to the treasury.
    pub slash_shares: Vec<u128>,
    /// allocation - slash, back to the auditor's free stake.
    pub auditor_return: u128,
}

impl Payouts {
    /// What victims received from the reserve.
    pub fn victim_paid(&self) -> u128 {
        self.victim_shares.iter().sum()
    }
    /// What victims received from the auditor's slash.
    pub fn slash_to_victims(&self) -> u128 {
        self.slash_shares.iter().sum()
    }
    pub fn fee_paid(&self) -> u128 {
        self.fee_shares.iter().sum()
    }
    /// Total that leaves the certificate's reserve.
    pub fn reserve_out(&self) -> u128 {
        self.reserve_draw + self.fee_pool
    }
    /// Total credited to the treasury.
    pub fn to_treasury(&self) -> u128 {
        self.victim_remainder + self.fee_remainder + (self.slash - self.slash_to_victims())
    }
}

fn min(a: u128, b: u128) -> u128 {
    if a < b { a } else { b }
}

/// Hand out `pool` in proportion to `weights` over `total`. Truncating
/// division, so the shares can sum to slightly less than `pool`.
pub fn distribute(pool: u128, weights: &[u128], total: u128) -> Vec<u128> {
    if pool == 0 || total == 0 {
        return vec![0; weights.len()];
    }
    weights
        .iter()
        .map(|w| {
            pool.checked_mul(*w)
                .expect("overflow in distribute")
                .checked_div(total)
                .expect("division by zero")
        })
        .collect()
}

/// The fraud-mode waterfall. Caller guarantees `claims` is non-empty and the
/// total weight is > 0 (otherwise the window settles in hygiene mode).
pub fn settle(reserve: u128, allocation: u128, claims: &[ClaimIn], fee_bps: u128) -> Payouts {
    let total_harm: u128 = claims.iter().map(|c| c.weight).sum();
    let collateral = reserve.checked_add(allocation).expect("overflow");
    let payable = min(total_harm, collateral);

    // 1. Victims, from the operator's own reserve, pro rata over the
    //    ARBITRATED harm only (R2: a predicate proof names no victim).
    let victim_weights: Vec<u128> = claims
        .iter()
        .map(|c| if c.arbitrated { c.weight } else { 0 })
        .collect();
    let arbitrated_harm: u128 = victim_weights.iter().sum();
    let reserve_draw = min(payable, reserve);
    let victim_pool = min(reserve_draw, arbitrated_harm);
    let victim_shares = distribute(victim_pool, &victim_weights, arbitrated_harm);
    let victim_paid: u128 = victim_shares.iter().sum();
    let victim_remainder = reserve_draw - victim_paid;
    let reserve_left = reserve - reserve_draw;

    // 2. Challenger fees: fee_bps of proven harm, out of what is left of the
    //    reserve, never out of the stake; pro rata by weight.
    let fee_target = total_harm.checked_mul(fee_bps).expect("overflow") / 10_000;
    let fee_pool = min(fee_target, reserve_left);
    let weights: Vec<u128> = claims.iter().map(|c| c.weight).collect();
    let fee_shares = distribute(fee_pool, &weights, total_harm);
    let fee_paid: u128 = fee_shares.iter().sum();
    let fee_remainder = fee_pool - fee_paid;

    // 3. Auditor slash: only the part of payable the reserve could not cover,
    //    capped by this certificate's allocation. It goes to the arbitrated
    //    victims' unpaid harm first, pro rata; the rest to the treasury.
    let residual = payable - reserve_draw;
    let slash = min(residual, allocation);
    let unpaid: Vec<u128> = victim_weights
        .iter()
        .zip(victim_shares.iter())
        .map(|(w, p)| w - p)
        .collect();
    let unpaid_total: u128 = unpaid.iter().sum();
    let slash_pool = min(slash, unpaid_total);
    let slash_shares = distribute(slash_pool, &unpaid, unpaid_total);

    // 5. The unslashed allocation goes back to the auditor's free stake.
    let auditor_return = allocation - slash;

    Payouts {
        payable,
        reserve_draw,
        victim_shares,
        victim_remainder,
        fee_pool,
        fee_shares,
        fee_remainder,
        slash,
        slash_shares,
        auditor_return,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const U: u128 = 1_000_000; // 1 USDC

    fn arb(w: u128) -> ClaimIn {
        ClaimIn {
            weight: w,
            arbitrated: true,
        }
    }

    /// Conservation: everything that leaves the pots is accounted for exactly.
    fn conserve(reserve: u128, allocation: u128, p: &Payouts) {
        assert_eq!(
            p.victim_paid() + p.victim_remainder,
            p.reserve_draw,
            "reserve draw conserved"
        );
        assert_eq!(
            p.fee_paid() + p.fee_remainder,
            p.fee_pool,
            "fee pool conserved"
        );
        assert!(p.reserve_out() <= reserve, "never more than the reserve");
        assert_eq!(
            p.slash + p.auditor_return,
            allocation,
            "allocation conserved"
        );
        assert!(p.slash_to_victims() <= p.slash, "slash conserved");
        assert!(p.reserve_draw + p.slash <= p.payable.max(p.reserve_draw + p.slash));
    }

    /// Soroban `cross_contract.rs` shape: harm below the reserve. The auditor
    /// is NOT slashed; the challenger fee comes out of the reserve.
    #[test]
    fn single_victim_harm_below_reserve_no_slash() {
        let p = settle(100 * U, 200 * U, &[arb(60 * U)], 1000);
        assert_eq!(p.payable, 60 * U);
        assert_eq!(p.reserve_draw, 60 * U);
        assert_eq!(p.victim_shares, vec![60 * U]);
        assert_eq!(p.fee_pool, 6 * U); // 10% of 60, reserve_left 40
        assert_eq!(p.fee_shares, vec![6 * U]);
        assert_eq!(p.slash, 0);
        assert_eq!(p.auditor_return, 200 * U);
        conserve(100 * U, 200 * U, &p);
    }

    /// The testnet demo vector: reserve 100, allocation 200, harm 120.
    #[test]
    fn harm_above_reserve_slashes_the_difference() {
        let p = settle(100 * U, 200 * U, &[arb(120 * U)], 1000);
        assert_eq!(p.payable, 120 * U);
        assert_eq!(p.reserve_draw, 100 * U);
        assert_eq!(p.victim_shares, vec![100 * U]);
        assert_eq!(p.fee_pool, 0); // min(12, 0)
        assert_eq!(p.slash, 20 * U);
        assert_eq!(p.slash_shares, vec![20 * U]); // the victim is made whole
        assert_eq!(p.auditor_return, 180 * U);
        assert_eq!(p.to_treasury(), 0);
        conserve(100 * U, 200 * U, &p);
    }

    #[test]
    fn harm_above_reserve_plus_allocation_is_capped() {
        let p = settle(100 * U, 200 * U, &[arb(1_000 * U)], 1000);
        assert_eq!(p.payable, 300 * U);
        assert_eq!(p.reserve_draw, 100 * U);
        assert_eq!(p.victim_shares, vec![100 * U]);
        assert_eq!(p.slash, 200 * U);
        assert_eq!(p.slash_shares, vec![200 * U]);
        assert_eq!(p.auditor_return, 0);
        conserve(100 * U, 200 * U, &p);
    }

    #[test]
    fn two_victims_pro_rata() {
        // harm 30 + 90 = 120 against reserve 100: victims split 100 as 25/75.
        let p = settle(100 * U, 200 * U, &[arb(30 * U), arb(90 * U)], 1000);
        assert_eq!(p.victim_shares, vec![25 * U, 75 * U]);
        assert_eq!(p.victim_remainder, 0);
        assert_eq!(p.slash, 20 * U);
        assert_eq!(p.slash_shares, vec![5 * U, 15 * U]); // unpaid 5 and 15
        assert_eq!(p.to_treasury(), 0);
        conserve(100 * U, 200 * U, &p);
    }

    #[test]
    fn two_victims_below_reserve_fees_pro_rata() {
        // harm 20 + 40 = 60: victims paid in full; fee 6 split 2/4.
        let p = settle(100 * U, 50 * U, &[arb(20 * U), arb(40 * U)], 1000);
        assert_eq!(p.victim_shares, vec![20 * U, 40 * U]);
        assert_eq!(p.fee_shares, vec![2 * U, 4 * U]);
        assert_eq!(p.slash, 0);
        conserve(100 * U, 50 * U, &p);
    }

    #[test]
    fn rounding_remainder_goes_to_treasury() {
        // 3 equal victims sharing 100 base units: 33 each, 1 to treasury.
        let p = settle(100, 0, &[arb(100), arb(100), arb(100)], 0);
        assert_eq!(p.reserve_draw, 100);
        assert_eq!(p.victim_shares, vec![33, 33, 33]);
        assert_eq!(p.victim_remainder, 1);
        conserve(100, 0, &p);
    }

    /// R2: a predicate (non-arbitrated) claim's share of the reserve draw goes
    /// to the treasury, not to the victim it named.
    #[test]
    fn predicate_share_goes_to_treasury_not_victim() {
        let claims = [
            arb(40 * U),
            ClaimIn {
                weight: 40 * U,
                arbitrated: false,
            },
        ];
        let p = settle(100 * U, 100 * U, &claims, 1000);
        assert_eq!(p.reserve_draw, 80 * U);
        assert_eq!(p.victim_shares, vec![40 * U, 0]);
        assert_eq!(p.victim_remainder, 40 * U);
        // fee: 10% of 80 = 8, split 4/4 between both challengers
        assert_eq!(p.fee_shares, vec![4 * U, 4 * U]);
        conserve(100 * U, 100 * U, &p);
    }

    /// The slash only tops up unpaid arbitrated harm; a predicate claim's
    /// part of the slash still goes to the treasury.
    #[test]
    fn slash_beyond_unpaid_harm_goes_to_treasury() {
        let claims = [
            arb(40 * U),
            ClaimIn {
                weight: 100 * U,
                arbitrated: false,
            },
        ];
        let p = settle(100 * U, 100 * U, &claims, 1000);
        assert_eq!(p.victim_shares, vec![40 * U, 0]);
        assert_eq!(p.slash, 40 * U);
        assert_eq!(p.slash_shares, vec![0, 0]);
        assert_eq!(p.to_treasury(), 60 * U + 40 * U);
        conserve(100 * U, 100 * U, &p);
    }

    #[test]
    fn zero_pool_distributes_nothing() {
        assert_eq!(distribute(0, &[1, 2], 3), vec![0, 0]);
        assert_eq!(distribute(5, &[0, 0], 0), vec![0, 0]);
    }
}
