#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contracttype]
pub enum DataKey {
    ChallengeManager,
    Token,
    Operator,
    Auditor,
    Amount,
    Released,
}

#[contract]
pub struct FeeEscrow;

#[contractimpl]
impl FeeEscrow {
    pub fn initialize(env: Env, challenge_manager: Address, token: Address) {
        if env.storage().instance().has(&DataKey::ChallengeManager) {
            panic!("already_initialized");
        }
        env.storage().instance().set(&DataKey::ChallengeManager, &challenge_manager);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Released, &false);
        env.storage().instance().set(&DataKey::Amount, &0i128);
    }

    // Operator deposits audit fee, specifies which auditor will receive it
    pub fn deposit(env: Env, operator: Address, auditor: Address, amount: i128) {
        operator.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &operator,
            &env.current_contract_address(),
            &amount,
        );

        env.storage().instance().set(&DataKey::Operator, &operator);
        env.storage().instance().set(&DataKey::Auditor, &auditor);
        env.storage().instance().set(&DataKey::Amount, &amount);
    }

    // Auditor calls this after attestation — collects their fee
    pub fn release_to_auditor(env: Env) {
        let auditor: Address = env.storage().instance().get(&DataKey::Auditor).unwrap();
        auditor.require_auth();

        let released: bool = env.storage().instance().get(&DataKey::Released).unwrap();
        if released {
            panic!("already_released");
        }

        let amount: i128 = env.storage().instance().get(&DataKey::Amount).unwrap();
        if amount == 0 {
            panic!("no_deposit");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &auditor,
            &amount,
        );

        env.storage().instance().set(&DataKey::Released, &true);
        env.storage().instance().set(&DataKey::Amount, &0i128);
    }

    // ChallengeManager slashes fee to challenger if auditor committed fraud
    pub fn slash_to_challenger(env: Env, challenger: Address) {
        let cm: Address = env.storage().instance().get(&DataKey::ChallengeManager).unwrap();
        cm.require_auth();

        let amount: i128 = env.storage().instance().get(&DataKey::Amount).unwrap();
        if amount == 0 {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &challenger,
            &amount,
        );

        env.storage().instance().set(&DataKey::Amount, &0i128);
    }

    pub fn get_amount(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Amount).unwrap_or(0)
    }

    pub fn is_released(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Released).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    #[test]
    #[should_panic(expected = "already_released")]
    fn test_double_release_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let cm = Address::generate(&env);
        let token = Address::generate(&env);
        let auditor = Address::generate(&env);

        let contract_id = env.register(FeeEscrow, ());
        let client = FeeEscrowClient::new(&env, &contract_id);
        client.initialize(&cm, &token);

        // Simulate a completed deposit + first release by setting state directly
        env.as_contract(&contract_id, || {
            env.storage().instance().set(&DataKey::Auditor, &auditor);
            env.storage().instance().set(&DataKey::Amount, &500_0000000i128);
            env.storage().instance().set(&DataKey::Released, &true); // already released
        });

        client.release_to_auditor(); // should panic: already_released
    }

    #[test]
    #[should_panic(expected = "no_deposit")]
    fn test_release_without_deposit_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let cm = Address::generate(&env);
        let token = Address::generate(&env);
        let auditor = Address::generate(&env);

        let contract_id = env.register(FeeEscrow, ());
        let client = FeeEscrowClient::new(&env, &contract_id);
        client.initialize(&cm, &token);

        // Set auditor but no amount — simulates calling release before deposit
        env.as_contract(&contract_id, || {
            env.storage().instance().set(&DataKey::Auditor, &auditor);
        });

        client.release_to_auditor(); // should panic: no_deposit
    }
}
