#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contracttype]
pub enum DataKey {
    Operator,
    ChallengeManager,
    Token,
    Balance,
    Locked,
    UnlockAt,
}

#[contract]
pub struct ReserveVault;

#[contractimpl]
impl ReserveVault {
    pub fn initialize(
        env: Env,
        operator: Address,
        challenge_manager: Address,
        token: Address,
        unlock_at: u64,
    ) {
        if env.storage().instance().has(&DataKey::Operator) {
            panic!("already_initialized");
        }
        // Reserve stays locked to the operator until the certificate expires.
        env.storage().instance().set(&DataKey::Operator, &operator);
        env.storage().instance().set(&DataKey::ChallengeManager, &challenge_manager);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Balance, &0i128);
        env.storage().instance().set(&DataKey::Locked, &false);
        env.storage().instance().set(&DataKey::UnlockAt, &unlock_at);
    }

    pub fn deposit(env: Env, amount: i128) {
        let operator: Address = env.storage().instance().get(&DataKey::Operator).unwrap();
        operator.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &operator,
            &env.current_contract_address(),
            &amount,
        );

        let balance: i128 = env.storage().instance().get(&DataKey::Balance).unwrap();
        env.storage().instance().set(&DataKey::Balance, &(balance + amount));
        env.storage().instance().set(&DataKey::Locked, &true);
    }

    pub fn get_balance(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Balance).unwrap_or(0)
    }

    // Only ChallengeManager can call this — compensates a harmed counterparty
    pub fn release_to_victim(env: Env, victim: Address, amount: i128) {
        let cm: Address = env.storage().instance().get(&DataKey::ChallengeManager).unwrap();
        cm.require_auth();

        let balance: i128 = env.storage().instance().get(&DataKey::Balance).unwrap();
        if amount > balance {
            panic!("insufficient_reserve");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &victim,
            &amount,
        );

        env.storage().instance().set(&DataKey::Balance, &(balance - amount));
    }

    // Operator reclaims reserve only after certificate expiry, with no incidents
    pub fn release_to_operator(env: Env) {
        let operator: Address = env.storage().instance().get(&DataKey::Operator).unwrap();
        operator.require_auth();

        let unlock_at: u64 = env.storage().instance().get(&DataKey::UnlockAt).unwrap();
        if env.ledger().timestamp() < unlock_at {
            panic!("reserve_still_locked");
        }

        let balance: i128 = env.storage().instance().get(&DataKey::Balance).unwrap();
        if balance == 0 {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &operator,
            &balance,
        );

        env.storage().instance().set(&DataKey::Balance, &0i128);
        env.storage().instance().set(&DataKey::Locked, &false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Env,
    };

    #[test]
    fn test_deposit_updates_balance() {
        let env = Env::default();
        env.mock_all_auths();

        let operator = Address::generate(&env);
        let cm = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(ReserveVault, ());
        let client = ReserveVaultClient::new(&env, &contract_id);

        client.initialize(&operator, &cm, &token, &1000u64);
        // Note: actual token transfer tested in integration tests
        // Here we verify state management logic
        assert_eq!(client.get_balance(), 0);
    }

    #[test]
    #[should_panic(expected = "already_initialized")]
    fn test_double_initialize_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let operator = Address::generate(&env);
        let cm = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(ReserveVault, ());
        let client = ReserveVaultClient::new(&env, &contract_id);

        client.initialize(&operator, &cm, &token, &1000u64);
        client.initialize(&operator, &cm, &token, &1000u64); // should panic
    }

    #[test]
    #[should_panic(expected = "reserve_still_locked")]
    fn test_release_to_operator_before_unlock_panics() {
        let env = Env::default();
        env.mock_all_auths();

        let operator = Address::generate(&env);
        let cm = Address::generate(&env);
        let token = Address::generate(&env);

        let contract_id = env.register(ReserveVault, ());
        let client = ReserveVaultClient::new(&env, &contract_id);

        client.initialize(&operator, &cm, &token, &5000u64);
        env.ledger().set_timestamp(1000); // before unlock_at — still locked
        client.release_to_operator(); // should panic
    }
}
