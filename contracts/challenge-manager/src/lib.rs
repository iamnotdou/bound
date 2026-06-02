#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Val, Vec};

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
    MinStake,
    Challenge(u64),
    ChallengeCount,
}

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
        min_stake: i128,
    ) {
        if env.storage().instance().has(&DataKey::Registry) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage().instance().set(&DataKey::AuditorStaking, &auditor_staking);
        env.storage().instance().set(&DataKey::ReserveVault, &reserve_vault);
        env.storage().instance().set(&DataKey::FeeEscrow, &fee_escrow);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Arbiter, &arbiter);
        env.storage().instance().set(&DataKey::MinStake, &min_stake);
        env.storage().instance().set(&DataKey::ChallengeCount, &0u64);
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

        let count: u64 = env.storage().instance().get(&DataKey::ChallengeCount).unwrap();
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
        env.storage().instance().set(&DataKey::ChallengeCount, &challenge_id);

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

    pub fn get_challenge_count(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::ChallengeCount).unwrap_or(0)
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
        let reserve_vault: Address = env.storage().instance().get(&DataKey::ReserveVault).unwrap();

        let claimed: i128 = env.invoke_contract(
            &registry,
            &Symbol::new(env, "get_cert_reserve"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        );
        let actual: i128 = env.invoke_contract(
            &reserve_vault,
            &Symbol::new(env, "get_balance"),
            Vec::<Val>::new(env),
        );

        actual < claimed
    }

    // Fraud proven: slash the auditor's stake, compensate the victim from the
    // slashed stake + the reserve, reward the challenger, invalidate the cert,
    // and return the challenger's bond. All on-chain, one transaction.
    fn settle_fraud(env: &Env, challenge_id: u64, ch: &Challenge) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        let staking: Address = env.storage().instance().get(&DataKey::AuditorStaking).unwrap();
        let vault: Address = env.storage().instance().get(&DataKey::ReserveVault).unwrap();
        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();

        // Who vouched for this certificate?
        let auditor: Address = env.invoke_contract(
            &registry,
            &Symbol::new(env, "get_cert_auditor"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        // Slash the auditor's full live stake: bulk to the victim, a cut to the challenger.
        let stake: i128 = env.invoke_contract(
            &staking,
            &Symbol::new(env, "get_stake"),
            Vec::from_array(env, [auditor.clone().into_val(env)]),
        );
        let reward = stake / 5; // 20% finder's fee to the challenger
        let victim_share = stake - reward;

        if victim_share > 0 {
            env.invoke_contract::<()>(
                &staking,
                &Symbol::new(env, "slash"),
                Vec::from_array(
                    env,
                    [
                        auditor.clone().into_val(env),
                        ch.victim.clone().into_val(env),
                        victim_share.into_val(env),
                    ],
                ),
            );
        }
        if reward > 0 {
            env.invoke_contract::<()>(
                &staking,
                &Symbol::new(env, "slash"),
                Vec::from_array(
                    env,
                    [
                        auditor.into_val(env),
                        ch.challenger.clone().into_val(env),
                        reward.into_val(env),
                    ],
                ),
            );
        }

        // Drain whatever reserve remains to the victim.
        let reserve_bal: i128 = env.invoke_contract(
            &vault,
            &Symbol::new(env, "get_balance"),
            Vec::<Val>::new(env),
        );
        if reserve_bal > 0 {
            env.invoke_contract::<()>(
                &vault,
                &Symbol::new(env, "release_to_victim"),
                Vec::from_array(
                    env,
                    [ch.victim.clone().into_val(env), reserve_bal.into_val(env)],
                ),
            );
        }

        // Mark the certificate dead.
        env.invoke_contract::<()>(
            &registry,
            &Symbol::new(env, "invalidate"),
            Vec::from_array(env, [ch.cert_id.into_val(env)]),
        );

        // Return the challenger's bond.
        token::Client::new(env, &token_addr).transfer(
            &env.current_contract_address(),
            &ch.challenger,
            &ch.stake,
        );

        let mut resolved = ch.clone();
        resolved.verdict = Verdict::ChallengeWins;
        env.storage().persistent().set(&DataKey::Challenge(challenge_id), &resolved);
    }

    // Challenge failed: the challenger forfeits their bond (it stays in the contract).
    fn settle_no_fraud(env: &Env, challenge_id: u64) {
        let mut ch: Challenge = env
            .storage()
            .persistent()
            .get(&DataKey::Challenge(challenge_id))
            .expect("challenge_not_found");
        ch.verdict = Verdict::ChallengeFails;
        env.storage().persistent().set(&DataKey::Challenge(challenge_id), &ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn init_client(env: &Env) -> (ChallengeManagerClient, Address) {
        let registry = Address::generate(env);
        let auditor_staking = Address::generate(env);
        let reserve_vault = Address::generate(env);
        let fee_escrow = Address::generate(env);
        let token = Address::generate(env);
        let arbiter = Address::generate(env);

        let contract_id = env.register(ChallengeManager, ());
        let client = ChallengeManagerClient::new(env, &contract_id);
        client.initialize(
            &registry,
            &auditor_staking,
            &reserve_vault,
            &fee_escrow,
            &token,
            &arbiter,
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
