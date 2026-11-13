//! Mock USDC for the Bound NEAR testnet demo.
//!
//! A plain NEP-141 / NEP-145 / NEP-148 fungible token built on
//! `near-contract-standards`, 6 decimals, with an OPEN `mint`: anyone can mint
//! any amount to any registered account. It is play money for testnet only, so
//! a judge or the AI screener can fund demo actors without any faucet.
//! Never deploy this to mainnet.

use near_contract_standards::fungible_token::metadata::{
    FT_METADATA_SPEC, FungibleTokenMetadata, FungibleTokenMetadataProvider,
};
use near_contract_standards::fungible_token::{
    FungibleToken, FungibleTokenCore, FungibleTokenResolver,
};
use near_contract_standards::storage_management::{
    StorageBalance, StorageBalanceBounds, StorageManagement,
};
use near_sdk::json_types::U128;
use near_sdk::{
    AccountId, BorshStorageKey, NearToken, PanicOnDefault, PromiseOrValue, near, require,
};

/// Largest amount one `mint` call may create: 1,000,000 USDC. Keeps a typo from
/// overflowing total supply in a shared demo deployment.
pub const MAX_MINT: u128 = 1_000_000 * 1_000_000;

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Accounts,
}

#[derive(PanicOnDefault)]
#[near(contract_state)]
pub struct MockUsdc {
    token: FungibleToken,
}

#[near]
impl MockUsdc {
    #[init]
    pub fn new() -> Self {
        Self {
            token: FungibleToken::new(StorageKey::Accounts),
        }
    }

    /// Open mint (testnet play money). Registers `account_id` on the token if
    /// needed, paid by this contract (testnet only; bounded by one entry per
    /// call and the caller's gas).
    pub fn mint(&mut self, account_id: AccountId, amount: U128) {
        require!(amount.0 > 0 && amount.0 <= MAX_MINT, "invalid mint amount");
        if !self.token.accounts.contains_key(&account_id) {
            self.token.internal_register_account(&account_id);
        }
        self.token.internal_deposit(&account_id, amount.0);
        near_contract_standards::fungible_token::events::FtMint {
            owner_id: &account_id,
            amount,
            memo: Some("mock-usdc open mint"),
        }
        .emit();
    }
}

#[near]
impl FungibleTokenCore for MockUsdc {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>) {
        self.token.ft_transfer(receiver_id, amount, memo)
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        self.token.ft_transfer_call(receiver_id, amount, memo, msg)
    }

    fn ft_total_supply(&self) -> U128 {
        self.token.ft_total_supply()
    }

    fn ft_balance_of(&self, account_id: AccountId) -> U128 {
        self.token.ft_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenResolver for MockUsdc {
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128 {
        let (used, _burned) =
            self.token
                .internal_ft_resolve_transfer(&sender_id, receiver_id, amount);
        used.into()
    }
}

#[near]
impl StorageManagement for MockUsdc {
    #[payable]
    fn storage_deposit(
        &mut self,
        account_id: Option<AccountId>,
        registration_only: Option<bool>,
    ) -> StorageBalance {
        self.token.storage_deposit(account_id, registration_only)
    }

    #[payable]
    fn storage_withdraw(&mut self, amount: Option<NearToken>) -> StorageBalance {
        self.token.storage_withdraw(amount)
    }

    #[payable]
    fn storage_unregister(&mut self, force: Option<bool>) -> bool {
        self.token.internal_storage_unregister(force).is_some()
    }

    fn storage_balance_bounds(&self) -> StorageBalanceBounds {
        self.token.storage_balance_bounds()
    }

    fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.token.storage_balance_of(account_id)
    }
}

#[near]
impl FungibleTokenMetadataProvider for MockUsdc {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Bound Mock USDC (testnet play money)".to_string(),
            symbol: "mUSDC".to_string(),
            icon: None,
            reference: None,
            reference_hash: None,
            decimals: 6,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use near_sdk::test_utils::{VMContextBuilder, accounts};
    use near_sdk::{env, testing_env};

    fn ctx(predecessor: AccountId) -> VMContextBuilder {
        let mut b = VMContextBuilder::new();
        b.current_account_id("usdc.test".parse().unwrap())
            .predecessor_account_id(predecessor);
        b
    }

    #[test]
    fn mint_credits_balance_and_supply() {
        testing_env!(ctx(accounts(0)).build());
        let mut t = MockUsdc::new();
        t.mint(accounts(1), U128(5_000_000));
        assert_eq!(t.ft_balance_of(accounts(1)).0, 5_000_000);
        assert_eq!(t.ft_total_supply().0, 5_000_000);
        assert_eq!(t.ft_metadata().decimals, 6);
    }

    #[test]
    #[should_panic(expected = "invalid mint amount")]
    fn mint_zero_rejected() {
        testing_env!(ctx(accounts(0)).build());
        let mut t = MockUsdc::new();
        t.mint(accounts(1), U128(0));
    }

    #[test]
    #[should_panic(expected = "invalid mint amount")]
    fn mint_above_cap_rejected() {
        testing_env!(ctx(accounts(0)).build());
        let mut t = MockUsdc::new();
        t.mint(accounts(1), U128(MAX_MINT + 1));
    }

    #[test]
    fn unused_env_is_fine() {
        testing_env!(ctx(accounts(0)).build());
        assert_eq!(env::current_account_id().as_str(), "usdc.test");
    }
}
