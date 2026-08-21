#![no_std]
// `initialize` takes 8 arguments, one over clippy's default threshold. The argument
// list of a `pub fn` in a #[contractimpl] block is the contract's on-chain ABI:
// changing it would change the generated bindings and force a redeploy to a new
// address. The lint has to be silenced at crate level rather than on the impl
// block, because #[contractimpl] re-emits the signature as sibling items that an
// item-level allow does not cover.
#![allow(clippy::too_many_arguments)]
// USDC amounts are written as <dollars>_<7 decimals>, e.g. 10_0000000 is $10.
// Clippy reads that as inconsistent grouping; the grouping is deliberate so the
// dollar figure stays legible.
#![allow(clippy::inconsistent_digit_grouping)]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum ProofType {
    // Kategori A — kontrat kendisi doğrular (trustless)
    InsufficientReserve,
    // Kategori B — arbiter verdict verir (on-chain kanıtlanamaz)
    BoundExceeded,
    FakeSignature,
    ExpiredCertificate,
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum Verdict {
    Pending,
    ChallengeWins,
    ChallengeFails,
}

#[contracttype]
#[derive(Clone)]
pub struct Challenge {
    pub challenger: Address,
    pub cert_id: u64,
    pub proof_type: ProofType,
    pub victim: Address,
    pub stake: i128,
    pub verdict: Verdict,
}

#[contracttype]
pub enum DataKey {
    Registry,
    AuditorStaking,
    ReserveVault,
    FeeEscrow,
    Token,
    Arbiter,
    /// Where slashed stake goes, and the only place it may go.
    Treasury,
    MinStake,
    Challenge(u64),
    ChallengeCount,
    /// Sum of challenger bonds currently held and still owed back. Anything the
    /// contract holds above this is forfeited-bond surplus, which is what funds
    /// the hygiene bounty.
    BondsHeld,
}

/// The challenger's fee, in basis points of **proven harm**.
///
/// Deliberately a share of harm and not of the auditor's stake. v1 paid 20% of
/// the live stake, which made hunting *auditors* profitable rather than hunting
/// *fraud*: the payout scaled with how much collateral the auditor happened to
/// have, not with what anyone lost. Anchoring it to harm means a challenge is
/// worth filing exactly in proportion to the damage it surfaces.
const CHALLENGER_FEE_BPS: i128 = 1_000; // 10%

/// Flat bounty for a proof that is true but that nobody can evidence harm for
/// (see `settle_hygiene`). Fixed, small, and never scaled by the auditor's
/// stake — its job is to pay for the gas of killing a dead certificate, not to
/// fund a hunting expedition.
const HYGIENE_BOUNTY: i128 = 10_0000000; // $10

#[contract]
pub struct ChallengeManager;

#[contractimpl]
impl ChallengeManager {
    pub fn initialize(
        env: Env,
        registry: Address,
        auditor_staking: Address,
        reserve_vault: Address,
        fee_escrow: Address,
        token: Address,
        arbiter: Address,
        treasury: Address,
        min_stake: i128,
    ) {
        if env.storage().instance().has(&DataKey::Registry) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage()
            .instance()
            .set(&DataKey::AuditorStaking, &auditor_staking);
        env.storage()
            .instance()
            .set(&DataKey::ReserveVault, &reserve_vault);
        env.storage()
            .instance()
            .set(&DataKey::FeeEscrow, &fee_escrow);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        // Named once, at initialize. No admin and no upgrade path: making the
        // treasury mutable would reopen the prize this whole waterfall exists to
        // remove.
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage().instance().set(&DataKey::MinStake, &min_stake);
        env.storage().instance().set(&DataKey::BondsHeld, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::ChallengeCount, &0u64);
    }

    // Anyone can submit a challenge — must post a bond to prevent spam.
    // `victim` is the counterparty to be compensated if fraud is proven.
    pub fn challenge(
        env: Env,
        challenger: Address,
        cert_id: u64,
        proof_type: ProofType,
        victim: Address,
        stake: i128,
    ) -> u64 {
        challenger.require_auth();

        let min_stake: i128 = env.storage().instance().get(&DataKey::MinStake).unwrap();
        if stake < min_stake {
            panic!("stake_below_minimum");
        }

        // Bond moves into the ChallengeManager and stays at risk until resolution
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &challenger,
            &env.current_contract_address(),
            &stake,
        );

        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeCount)
            .unwrap();
        let challenge_id = count + 1;

        env.storage().persistent().set(
            &DataKey::Challenge(challenge_id),
            &Challenge {
                challenger,
                cert_id,
                proof_type,
                victim,
                stake,
                verdict: Verdict::Pending,
            },
        );
        env.storage()
            .instance()
            .set(&DataKey::ChallengeCount, &challenge_id);
        env.storage()
            .instance()
            .set(&DataKey::BondsHeld, &(Self::bonds_held(&env) + stake));

        challenge_id
    }

    // Trustless resolution. The contract proves the fraud itself by reading
    // on-chain state — no oracle, no human. Only valid for proof types that
    // are objectively verifiable on-chain (InsufficientReserve).
    pub fn resolve(env: Env, challenge_id: u64) {
        let ch = Self::load_pending(&env, challenge_id);

        let fraud = match ch.proof_type {
            ProofType::InsufficientReserve => Self::verify_insufficient_reserve(&env, ch.cert_id),
            // Everything else cannot be proven from contract state alone.
            _ => panic!("needs_arbiter"),
        };

        if fraud {
            Self::settle_fraud(&env, challenge_id, &ch);
        } else {
            Self::settle_no_fraud(&env, challenge_id);
        }
    }

    // Arbiter-gated resolution for subjective proof types (BoundExceeded,
    // FakeSignature) that no contract can verify on-chain. This is an explicit
    // trust assumption: the arbiter is named at initialize().
    pub fn resolve_by_arbiter(env: Env, challenge_id: u64, fraud_proven: bool) {
        let arbiter: Address = env.storage().instance().get(&DataKey::Arbiter).unwrap();
        arbiter.require_auth();

        let ch = Self::load_pending(&env, challenge_id);

        if fraud_proven {
            Self::settle_fraud(&env, challenge_id, &ch);
        } else {
            Self::settle_no_fraud(&env, challenge_id);
        }
    }

    pub fn get_challenge(env: Env, challenge_id: u64) -> Challenge {
        env.storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found")
    }

    pub fn get_treasury(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Treasury).unwrap()
    }

    /// Forfeited bonds the contract holds beyond what it still owes back. This
    /// is the only pot the hygiene bounty is paid from.
    pub fn get_bounty_pool(env: Env) -> i128 {
        Self::bounty_pool(&env)
    }

    pub fn get_bonds_held(env: Env) -> i128 {
        Self::bonds_held(&env)
    }

    pub fn get_challenge_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::ChallengeCount)
            .unwrap_or(0)
    }

    // ----- internals -----

    fn load_pending(env: &Env, challenge_id: u64) -> Challenge {
        let ch: Challenge = env
            .storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found");
        if ch.verdict != Verdict::Pending {
            panic!("already_resolved");
        }
        ch
    }

    // On-chain proof: the live reserve balance is below what the certificate claims.
    fn verify_insufficient_reserve(env: &Env, cert_id: u64) -> bool {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        let reserve_vault: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReserveVault)
            .unwrap();

        let claimed: i128 = env.invoke_contract(
            &registry,
            &Symbol::new(env, "get_cert_reserve"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );
        // Per-certificate accounting: the proof compares the certificate's own
        // claim against the certificate's own reserve. A deposit made for any
        // other certificate is invisible here.
        let actual: i128 = env.invoke_contract(
            &reserve_vault,
            &Symbol::new(env, "get_balance"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );

        actual < claimed
    }

    // ----- the settlement waterfall -------------------------------------
    //
    // ONE rule, applied identically to every proof type:
    //
    //     harm    = raw harm from the predicate
    //     payable = min(harm, reserve_for_this_cert + allocation_for_this_cert)
    //
    //     1. victim compensation  <- the operator's OWN reserve for THIS cert
    //     2. challenger fee       <- the same reserve, % of PROVEN HARM
    //     3. auditor slash        -> the TREASURY, never victim or challenger
    //     4. (premium step — not built; see the gap note in `settle_fraud`)
    //     5. allocation retires; unslashed remainder returns to free stake
    //     6. certificate invalidated; challenger's bond returned
    //
    // Every line closes a specific attack. Before "simplifying" any of them,
    // read the reason attached to it — v1 had none of these and paid a
    // colluding operator the auditor's entire bond for the price of one lie.

    /// Raw harm the predicate proves, in stroops. Zero means "the covenant was
    /// broken but no harm is evidenced on-chain" — see `settle_hygiene`.
    ///
    /// `InsufficientReserve`: the shortfall, claimed reserve minus actual. The
    /// named victim is deliberately NOT part of this number. A named victim is a
    /// filter, not a proof: receiving a payment is evidence of being paid, not
    /// of being harmed. The waterfall neutralises a self-named victim rather
    /// than the predicate trying to authenticate one.
    fn raw_harm(env: &Env, ch: &Challenge) -> i128 {
        match ch.proof_type {
            ProofType::InsufficientReserve => {
                let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
                let vault: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ReserveVault)
                    .unwrap();
                let claimed: i128 = env.invoke_contract(
                    &registry,
                    &Symbol::new(env, "get_cert_reserve"),
                    Vec::from_array(env, [ch.cert_id.into_val(env)]),
                );
                let actual: i128 = env.invoke_contract(
                    &vault,
                    &Symbol::new(env, "get_balance"),
                    Vec::from_array(env, [ch.cert_id.into_val(env)]),
                );
                if claimed > actual {
                    claimed - actual
                } else {
                    0
                }
            }
            // GAP: the arbiter path carries a verdict, not a quantity. Nothing
            // on-chain tells this contract how much a BoundExceeded or
            // FakeSignature ruling cost anyone, so those settle in hygiene mode:
            // the certificate dies, the challenger gets the flat bounty, and no
            // payout is invented from a number nobody proved. Giving the arbiter
            // a harm figure to sign is a separate decision, deliberately not
            // taken here.
            _ => 0,
        }
    }

    fn settle_fraud(env: &Env, challenge_id: u64, ch: &Challenge) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        let staking: Address = env
            .storage()
            .instance()
            .get(&DataKey::AuditorStaking)
            .unwrap();
        let vault: Address = env
            .storage()
            .instance()
            .get(&DataKey::ReserveVault)
            .unwrap();
        let treasury: Address = env.storage().instance().get(&DataKey::Treasury).unwrap();

        let harm = Self::raw_harm(env, ch);

        // The two pots this certificate can be settled against, and nothing
        // else. Both are keyed by cert_id: no other certificate's reserve and no
        // other certificate's allocation is reachable from here.
        let reserve: i128 = env.invoke_contract(
            &vault,
            &Symbol::new(env, "get_balance"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );
        let allocation: i128 = env.invoke_contract(
            &staking,
            &Symbol::new(env, "get_allocation"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        // Nothing provable to pay for: kill the certificate, pay the flat
        // bounty, leave the money where it is.
        if harm <= 0 {
            Self::settle_hygiene(env, challenge_id, ch, &registry, &staking);
            return;
        }

        // The ceiling on everything below. Total outflow can never exceed the
        // harm actually proven, nor the collateral this certificate carries.
        let payable = if harm < reserve + allocation {
            harm
        } else {
            reserve + allocation
        };

        // 1. Victim compensation — from the OPERATOR'S OWN RESERVE for THIS
        //    certificate only.
        //
        //    ATTACK CLOSED: self-dealing. A colluding operator that names an
        //    address it controls as "victim" is moving money from its left
        //    pocket to its right. Manufacturing a proof against yourself
        //    extracts nothing, so the permissiveness of victim naming stops
        //    mattering.
        let victim_amount = if payable < reserve { payable } else { reserve };
        if victim_amount > 0 {
            Self::pay_from_reserve(env, &vault, ch.cert_id, &ch.victim, victim_amount);
        }
        let reserve_left = reserve - victim_amount;

        // 2. Challenger fee — a percentage of PROVEN HARM, out of the same
        //    reserve, never out of the stake.
        //
        //    ATTACK CLOSED: bounty-hunting auditors. v1 paid 20% of the auditor's
        //    live stake, so the most profitable target was whoever had posted the
        //    most collateral, regardless of how small the fraud was.
        let mut fee = harm * CHALLENGER_FEE_BPS / 10_000;
        if fee > reserve_left {
            fee = reserve_left;
        }
        if fee > 0 {
            Self::pay_from_reserve(env, &vault, ch.cert_id, &ch.challenger, fee);
        }

        // 3. Auditor slash — to the TREASURY, and only the treasury.
        //
        //    ATTACK CLOSED: the prize. Nobody who can trigger a proof can
        //    receive the auditor's money, so manufacturing a true proof pays
        //    nothing however true it is.
        //
        //    The draw is the harm the operator's own reserve could not cover,
        //    capped by this certificate's allocation.
        //
        //    ATTACK CLOSED: disproportion. A manufactured $10 breach cannot cost
        //    an auditor a $50,000 bond — it is capped by harm — and one bad
        //    certificate cannot destroy an auditor's whole book, because it is
        //    capped by that certificate's allocation.
        let residual = payable - victim_amount;
        let slash = if residual < allocation {
            residual
        } else {
            allocation
        };
        if slash > 0 {
            env.invoke_contract::<()>(
                &staking,
                &Symbol::new(env, "slash_allocation"),
                Vec::from_array(
                    env,
                    [
                        ch.cert_id.into_val(env),
                        treasury.into_val(env),
                        slash.into_val(env),
                    ],
                ),
            );
        }

        // 4. GAP — premium step. DESIGN-V2's premium economy would take a cut
        //    here, before the allocation retires, and route it to whoever
        //    underwrote the coverage. No PremiumVault exists yet, so nothing is
        //    taken and nothing is escrowed for one. When it lands it goes here,
        //    between the slash and the retirement, so that the amounts above are
        //    unaffected by its arrival.

        // 5. Retire the allocation: the unslashed remainder goes back to the
        //    auditor's FREE stake.
        //
        //    ATTACK CLOSED (against the auditor, by omission): stranded capital.
        //    Without this the remainder would stay allocated to a dead
        //    certificate forever — which is exactly the defect the
        //    per-certificate refactor exists to remove, quietly reintroduced.
        env.invoke_contract::<()>(
            &staking,
            &Symbol::new(env, "retire_allocation"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        // 6. Kill the certificate and return the challenger's bond.
        Self::invalidate(env, &registry, ch.cert_id);
        Self::return_bond(env, ch);

        Self::record(env, challenge_id, ch, Verdict::ChallengeWins);
    }

    /// Hygiene mode: the proof is real, but nobody can evidence harm.
    ///
    /// The certificate is invalidated, the allocation retires in full, the
    /// challenger is paid a FIXED bounty out of forfeited bonds, and the
    /// operator's reserve is not touched at all. A dead certificate dies without
    /// a payout being invented for it.
    fn settle_hygiene(
        env: &Env,
        challenge_id: u64,
        ch: &Challenge,
        registry: &Address,
        staking: &Address,
    ) {
        env.invoke_contract::<()>(
            staking,
            &Symbol::new(env, "retire_allocation"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        Self::invalidate(env, registry, ch.cert_id);
        Self::return_bond(env, ch);

        // Paid from forfeited bonds only — failed challengers fund successful
        // hygiene challenges. There is no other pot it could come from: the
        // reserve is off limits by definition here, and paying it out of the
        // stake would hand a challenger a slice of the auditor's money, which is
        // the thing rule 3 exists to forbid. If the pool is short, the bounty is
        // whatever the pool holds, including nothing.
        let pool = Self::bounty_pool(env);
        let bounty = if HYGIENE_BOUNTY < pool {
            HYGIENE_BOUNTY
        } else {
            pool
        };
        if bounty > 0 {
            let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
            token::Client::new(env, &token_addr).transfer(
                &env.current_contract_address(),
                &ch.challenger,
                &bounty,
            );
        }

        Self::record(env, challenge_id, ch, Verdict::ChallengeWins);
    }

    // Challenge failed: the challenger forfeits their bond. It stays in the
    // contract and stops being owed back, which is what makes it available as
    // hygiene bounty for a future, genuine challenge.
    fn settle_no_fraud(env: &Env, challenge_id: u64) {
        let mut ch: Challenge = env
            .storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found");
        ch.verdict = Verdict::ChallengeFails;
        env.storage()
            .persistent()
            .set(&DataKey::Challenge(challenge_id), &ch);
        Self::release_bond_liability(env, ch.stake);
    }

    // ----- small helpers -------------------------------------------------

    fn pay_from_reserve(env: &Env, vault: &Address, cert_id: u64, to: &Address, amount: i128) {
        env.invoke_contract::<()>(
            vault,
            &Symbol::new(env, "pay_from_reserve"),
            Vec::from_array(
                env,
                [
                    cert_id.into_val(env),
                    to.clone().into_val(env),
                    amount.into_val(env),
                ],
            ),
        );
    }

    fn invalidate(env: &Env, registry: &Address, cert_id: u64) {
        env.invoke_contract::<()>(
            registry,
            &Symbol::new(env, "invalidate"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );
    }

    fn return_bond(env: &Env, ch: &Challenge) {
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(env, &token_addr).transfer(
            &env.current_contract_address(),
            &ch.challenger,
            &ch.stake,
        );
        Self::release_bond_liability(env, ch.stake);
    }

    fn release_bond_liability(env: &Env, amount: i128) {
        let held = Self::bonds_held(env) - amount;
        env.storage()
            .instance()
            .set(&DataKey::BondsHeld, &if held > 0 { held } else { 0 });
    }

    fn bonds_held(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::BondsHeld)
            .unwrap_or(0)
    }

    fn bounty_pool(env: &Env) -> i128 {
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let held = token::Client::new(env, &token_addr).balance(&env.current_contract_address());
        let owed = Self::bonds_held(env);
        if held > owed {
            held - owed
        } else {
            0
        }
    }

    fn record(env: &Env, challenge_id: u64, ch: &Challenge, verdict: Verdict) {
        let mut resolved = ch.clone();
        resolved.verdict = verdict;
        env.storage()
            .persistent()
            .set(&DataKey::Challenge(challenge_id), &resolved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn init_client(env: &Env) -> (ChallengeManagerClient<'_>, Address) {
        let registry = Address::generate(env);
        let auditor_staking = Address::generate(env);
        let reserve_vault = Address::generate(env);
        let fee_escrow = Address::generate(env);
        let token = Address::generate(env);
        let arbiter = Address::generate(env);
        let treasury = Address::generate(env);

        let contract_id = env.register(ChallengeManager, ());
        let client = ChallengeManagerClient::new(env, &contract_id);
        client.initialize(
            &registry,
            &auditor_staking,
            &reserve_vault,
            &fee_escrow,
            &token,
            &arbiter,
            &treasury,
            &100_0000000i128, // min stake $100
        );
        (client, contract_id)
    }

    #[test]
    #[should_panic(expected = "stake_below_minimum")]
    fn test_challenge_below_min_stake_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _) = init_client(&env);

        let challenger = Address::generate(&env);
        let victim = Address::generate(&env);
        // $10 < $100 min — panics before any token transfer
        client.challenge(
            &challenger,
            &1u64,
            &ProofType::InsufficientReserve,
            &victim,
            &10_0000000i128,
        );
    }

    #[test]
    #[should_panic(expected = "already_resolved")]
    fn test_double_resolve_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, contract_id) = init_client(&env);

        let challenger = Address::generate(&env);
        let victim = Address::generate(&env);

        // Seed a pending challenge directly (bypasses token transfer in challenge()).
        // Use an arbiter-path proof type so settle_no_fraud touches no other contract.
        env.as_contract(&contract_id, || {
            env.storage().persistent().set(
                &DataKey::Challenge(1u64),
                &Challenge {
                    challenger,
                    cert_id: 1,
                    proof_type: ProofType::BoundExceeded,
                    victim,
                    stake: 100_0000000,
                    verdict: Verdict::Pending,
                },
            );
        });

        // Arbiter rules "no fraud" → resolved. Second call must panic.
        client.resolve_by_arbiter(&1u64, &false);
        client.resolve_by_arbiter(&1u64, &false);
    }
}
