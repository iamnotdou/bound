#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

#[contracttype]
pub enum DataKey {
    Registry,
    ChallengeManager,
    Token,
    // Per-certificate reserve accounting. A single vault contract serves many
    // certificates, and each certificate's money is walled off from every other
    // one: funding cert A can never back cert B, and settling against A can
    // never draw down B.
    Balance(u64),
    Locked(u64),
    /// Ledger time at which this certificate's reserve may be reclaimed by its
    /// operator: the certificate's *settlement deadline*
    /// (`expires_at + CHALLENGE_WINDOW`), not its expiry. A proof about
    /// post-expiry activity only becomes provable after expiry; unlocking at
    /// `expires_at` would let the operator withdraw the reserve before the
    /// proof could ever be filed against it.
    UnlockAt(u64),
}

#[contract]
pub struct ReserveVault;

#[contractimpl]
impl ReserveVault {
    pub fn initialize(env: Env, registry: Address, challenge_manager: Address, token: Address) {
        if env.storage().instance().has(&DataKey::Registry) {
            panic!("already_initialized");
        }
        // The vault owns no operator of its own. Every certificate names its own
        // operator in the Registry, and that is the only address allowed to fund
        // or reclaim that certificate's reserve.
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage()
            .instance()
            .set(&DataKey::ChallengeManager, &challenge_manager);
        env.storage().instance().set(&DataKey::Token, &token);
    }

    pub fn deposit(env: Env, cert_id: u64, amount: i128) {
        if amount <= 0 {
            panic!("invalid_amount");
        }

        // Authenticate against *this certificate's* operator, read live from the
        // Registry. Any operator can fund their own certificate; nobody can fund
        // somebody else's.
        let operator = Self::cert_operator(&env, cert_id);
        operator.require_auth();

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &operator,
            &env.current_contract_address(),
            &amount,
        );

        let balance = Self::balance_of(&env, cert_id)
            .checked_add(amount)
            .expect("reserve_overflow");
        Self::set_balance(&env, cert_id, balance);

        // Reserve stays locked until the certificate's challenge window closes.
        env.storage()
            .persistent()
            .set(&DataKey::Locked(cert_id), &true);
        env.storage().persistent().set(
            &DataKey::UnlockAt(cert_id),
            &Self::cert_settlement_deadline(&env, cert_id),
        );
    }

    pub fn get_balance(env: Env, cert_id: u64) -> i128 {
        Self::balance_of(&env, cert_id)
    }

    pub fn is_locked(env: Env, cert_id: u64) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Locked(cert_id))
            .unwrap_or(false)
    }

    pub fn get_unlock_at(env: Env, cert_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::UnlockAt(cert_id))
            .unwrap_or(0)
    }

    /// Only the ChallengeManager can call this — it pays out of the reserve of
    /// the certificate under challenge, and no other.
    ///
    /// Both settlement payments run through here: the victim's compensation and
    /// the challenger's fee. Both are drawn from **the operator's own reserve
    /// for this certificate**, which is what makes self-dealing a wash — a
    /// colluding operator paying its own colluding "victim" is moving money
    /// from its left pocket to its right.
    pub fn pay_from_reserve(env: Env, cert_id: u64, to: Address, amount: i128) {
        let cm: Address = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeManager)
            .unwrap();
        cm.require_auth();

        if amount <= 0 {
            panic!("invalid_amount");
        }

        let balance = Self::balance_of(&env, cert_id);
        if amount > balance {
            panic!("insufficient_reserve");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &to,
            &amount,
        );

        Self::set_balance(&env, cert_id, balance - amount);
    }

    /// Operator reclaims this certificate's reserve only after its challenge
    /// window has closed — `expires_at + CHALLENGE_WINDOW`, not `expires_at`.
    pub fn release_to_operator(env: Env, cert_id: u64) {
        let operator = Self::cert_operator(&env, cert_id);
        operator.require_auth();

        // The deposit-time snapshot is a floor, not the answer. DESIGN-V2 §1
        // lets an open claim window push the certificate's settlement deadline
        // out past `expires_at + CHALLENGE_WINDOW`, and a snapshot taken when
        // the reserve was funded cannot know about a window opened later. So
        // the live deadline is read every time and the later of the two wins:
        // the operator cannot withdraw the collateral that a live window is
        // about to settle against.
        let snapshot: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::UnlockAt(cert_id))
            .unwrap_or(0);
        let live = Self::cert_settlement_deadline(&env, cert_id);
        let unlock_at = if snapshot > live { snapshot } else { live };
        if env.ledger().timestamp() < unlock_at {
            panic!("reserve_still_locked");
        }

        let balance = Self::balance_of(&env, cert_id);
        if balance == 0 {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &operator,
            &balance,
        );

        Self::set_balance(&env, cert_id, 0);
        env.storage()
            .persistent()
            .set(&DataKey::Locked(cert_id), &false);
    }

    // ----- internals -----

    fn balance_of(env: &Env, cert_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(cert_id))
            .unwrap_or(0)
    }

    fn set_balance(env: &Env, cert_id: u64, amount: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::Balance(cert_id), &amount);
    }

    fn registry(env: &Env) -> Address {
        env.storage().instance().get(&DataKey::Registry).unwrap()
    }

    fn cert_operator(env: &Env, cert_id: u64) -> Address {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_operator"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    /// The Registry owns the challenge window, so both the reserve here and the
    /// auditor's allocation in AuditorStaking unlock at exactly the same instant.
    fn cert_settlement_deadline(env: &Env, cert_id: u64) -> u64 {
        env.invoke_contract(
            &Self::registry(env),
            &Symbol::new(env, "get_cert_settlement_deadline"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }
}

#[cfg(test)]
mod mock_registry {
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    pub enum MockKey {
        Operator(u64),
        ExpiresAt(u64),
    }

    #[contract]
    pub struct MockRegistry;

    #[contractimpl]
    impl MockRegistry {
        pub fn set_cert(env: Env, cert_id: u64, operator: Address, expires_at: u64) {
            env.storage()
                .persistent()
                .set(&MockKey::Operator(cert_id), &operator);
            env.storage()
                .persistent()
                .set(&MockKey::ExpiresAt(cert_id), &expires_at);
        }

        pub fn get_cert_operator(env: Env, cert_id: u64) -> Address {
            env.storage()
                .persistent()
                .get(&MockKey::Operator(cert_id))
                .expect("certificate_not_found")
        }

        pub fn get_cert_settlement_deadline(env: Env, cert_id: u64) -> u64 {
            env.storage()
                .persistent()
                .get(&MockKey::ExpiresAt(cert_id))
                .expect("certificate_not_found")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mock_registry::{MockRegistry, MockRegistryClient};
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger as _},
        Env,
    };

    fn setup(env: &Env) -> (ReserveVaultClient<'_>, MockRegistryClient<'_>, Address) {
        let cm = Address::generate(env);
        let token = Address::generate(env);
        let registry_id = env.register(MockRegistry, ());
        let contract_id = env.register(ReserveVault, ());
        let client = ReserveVaultClient::new(env, &contract_id);
        client.initialize(&registry_id, &cm, &token);
        (client, MockRegistryClient::new(env, &registry_id), cm)
    }

    #[test]
    fn test_balance_starts_at_zero_per_certificate() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _registry, _cm) = setup(&env);

        // Note: actual token transfer tested in integration tests.
        // Here we verify state management logic.
        assert_eq!(client.get_balance(&1u64), 0);
        assert_eq!(client.get_balance(&2u64), 0);
        assert!(!client.is_locked(&1u64));
    }

    #[test]
    #[should_panic(expected = "already_initialized")]
    fn test_double_initialize_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, _registry, cm) = setup(&env);

        let token = Address::generate(&env);
        client.initialize(&cm, &cm, &token); // should panic
    }

    #[test]
    #[should_panic(expected = "reserve_still_locked")]
    fn test_release_to_operator_before_unlock_panics() {
        let env = Env::default();
        env.mock_all_auths();
        let (client, registry, _cm) = setup(&env);

        let operator = Address::generate(&env);
        registry.set_cert(&1u64, &operator, &5000u64);

        env.ledger().set_timestamp(1000); // before unlock_at — still locked
        client.release_to_operator(&1u64); // should panic
    }
}
