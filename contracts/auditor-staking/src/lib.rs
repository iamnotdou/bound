#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token, Address, Env, IntoVal, Symbol, Vec,
};

// Storage lifetime — defect L2. Soroban archives instance and persistent
// entries whose TTL lapses, and reaching an archived entry aborts the
// transaction rather than returning a default. Nothing here extended any TTL.
// 17,280 ledgers is one day at the ~5s close; the full reasoning for 120 days
// of runway, a threshold at half of it, and who pays the rent lives on the same
// two constants in `contracts/registry/src/lib.rs`.
const LEDGERS_PER_DAY: u32 = 17_280;
pub const TTL_THRESHOLD: u32 = 60 * LEDGERS_PER_DAY;
pub const TTL_EXTEND_TO: u32 = 120 * LEDGERS_PER_DAY;

#[contracttype]
pub enum DataKey {
    ChallengeManager,
    Registry,
    Token,
    MinRegistrationStake,
    /// Total capital this auditor has deposited: free + allocated. This is the
    /// number the contract actually custodies for them.
    Stake(Address),
    /// Sum of every live per-certificate allocation this auditor holds.
    /// `free = Stake - Allocated`, and free stake is what may be allocated to a
    /// new certificate or withdrawn.
    Allocated(Address),
    /// Per-certificate allocation, in the style the ReserveVault adopted: the
    /// slice of an auditor's stake that stands behind exactly one certificate.
    /// A slash draws against this and nothing else, so one bad certificate can
    /// never destroy an auditor's whole book.
    Allocation(u64),
    AllocationAuditor(u64),
    /// Ledger time at which this allocation may be freed by the auditor. Set by
    /// the Registry on attest to the certificate's *settlement deadline*
    /// (`expires_at + CHALLENGE_WINDOW`), not its expiry — see the comment on
    /// `allocate`.
    AllocationUnlockAt(u64),
    /// Informational: the latest unlock time across this auditor's allocations.
    LockedUntil(Address),
}

#[contract]
pub struct AuditorStaking;

#[contractimpl]
impl AuditorStaking {
    pub fn initialize(
        env: Env,
        challenge_manager: Address,
        registry: Address,
        token: Address,
        min_stake: i128,
    ) {
        if env.storage().instance().has(&DataKey::ChallengeManager) {
            panic!("already_initialized");
        }
        env.storage()
            .instance()
            .set(&DataKey::ChallengeManager, &challenge_manager);
        // Only the Registry may allocate an auditor's stake to a certificate
        // (on attest).
        env.storage().instance().set(&DataKey::Registry, &registry);
        env.storage().instance().set(&DataKey::Token, &token);
        Self::bump_instance(&env);
        env.storage()
            .instance()
            .set(&DataKey::MinRegistrationStake, &min_stake);
    }

    /// Deposit USDC into the staking contract. Deposited capital starts entirely
    /// free; it only goes at risk when it is allocated to a certificate.
    pub fn stake(env: Env, auditor: Address, amount: i128) {
        auditor.require_auth();

        if amount <= 0 {
            panic!("invalid_amount");
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &auditor,
            &env.current_contract_address(),
            &amount,
        );

        let current = Self::stake_of(&env, &auditor);
        env.storage()
            .persistent()
            .set(&DataKey::Stake(auditor.clone()), &(current + amount));

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Stake(auditor.clone()));
        Self::bump_if_present(&env, &DataKey::Allocated(auditor));
    }

    /// Total capital custodied for this auditor: free + allocated.
    pub fn get_stake(env: Env, auditor: Address) -> i128 {
        Self::stake_of(&env, &auditor)
    }

    /// Capital not currently standing behind any certificate. This is what can
    /// be allocated to a new attestation or withdrawn.
    pub fn get_free_stake(env: Env, auditor: Address) -> i128 {
        Self::stake_of(&env, &auditor) - Self::allocated_of(&env, &auditor)
    }

    pub fn get_allocated(env: Env, auditor: Address) -> i128 {
        Self::allocated_of(&env, &auditor)
    }

    /// Registration is judged on **free** stake, because the thing an auditor
    /// needs before vouching for one more certificate is capital that is not
    /// already vouching for another one. Under the old global-stake model this
    /// read the whole book, so a single $500 stake could back an unlimited
    /// number of certificates at $500 of advertised collateral each.
    pub fn is_registered(env: Env, auditor: Address) -> bool {
        let min: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinRegistrationStake)
            .unwrap_or(0);
        Self::stake_of(&env, &auditor) - Self::allocated_of(&env, &auditor) >= min
    }

    pub fn get_min_stake(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinRegistrationStake)
            .unwrap_or(0)
    }

    /// Allocate `amount` of an auditor's free stake to one certificate, locked
    /// until `until`. Only the Registry may call this — it does so inside
    /// `attest`, so the moment an auditor vouches, a named slice of their
    /// capital is at risk for that certificate and cannot be pulled out from
    /// under the counterparty.
    ///
    /// `until` is the certificate's **settlement deadline**
    /// (`expires_at + CHALLENGE_WINDOW`), not its expiry. A proof about
    /// post-expiry activity only becomes provable after expiry; if the
    /// allocation unlocked at `expires_at` the proof would settle against an
    /// already-freed allocation every single time.
    pub fn allocate(env: Env, auditor: Address, cert_id: u64, amount: i128, until: u64) {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        registry.require_auth();

        if amount <= 0 {
            panic!("invalid_amount");
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Allocation(cert_id))
        {
            panic!("already_allocated");
        }

        let min: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinRegistrationStake)
            .unwrap_or(0);
        if amount < min {
            panic!("allocation_below_minimum");
        }

        let free = Self::stake_of(&env, &auditor) - Self::allocated_of(&env, &auditor);
        if amount > free {
            panic!("insufficient_free_stake");
        }

        env.storage()
            .persistent()
            .set(&DataKey::Allocation(cert_id), &amount);
        env.storage()
            .persistent()
            .set(&DataKey::AllocationAuditor(cert_id), &auditor);
        env.storage()
            .persistent()
            .set(&DataKey::AllocationUnlockAt(cert_id), &until);
        env.storage().persistent().set(
            &DataKey::Allocated(auditor.clone()),
            &(Self::allocated_of(&env, &auditor) + amount),
        );

        let current: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::LockedUntil(auditor.clone()))
            .unwrap_or(0);
        if until > current {
            env.storage()
                .persistent()
                .set(&DataKey::LockedUntil(auditor.clone()), &until);
        }

        // All five entries have to outlive the certificate together. An
        // archived `AllocationAuditor` would abort every slash and every
        // release, freezing the allocation for good; an archived `Allocated`
        // would silently read back as zero the next time it was written from
        // scratch, handing the auditor free stake they never had.
        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Allocation(cert_id));
        Self::bump(&env, &DataKey::AllocationAuditor(cert_id));
        Self::bump(&env, &DataKey::AllocationUnlockAt(cert_id));
        Self::bump(&env, &DataKey::Allocated(auditor.clone()));
        Self::bump(&env, &DataKey::Stake(auditor.clone()));
        Self::bump_if_present(&env, &DataKey::LockedUntil(auditor));
    }

    pub fn get_allocation(env: Env, cert_id: u64) -> i128 {
        Self::allocation_of(&env, cert_id)
    }

    pub fn get_allocation_auditor(env: Env, cert_id: u64) -> Option<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::AllocationAuditor(cert_id))
    }

    pub fn allocation_unlock_at(env: Env, cert_id: u64) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::AllocationUnlockAt(cert_id))
            .unwrap_or(0)
    }

    /// Informational: the latest settlement deadline across this auditor's
    /// allocations. Nothing gates on it — an allocation is locked because it is
    /// allocated, not because of this timestamp.
    pub fn locked_until(env: Env, auditor: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::LockedUntil(auditor))
            .unwrap_or(0)
    }

    /// Slash a certificate's allocation to the **treasury**, and nowhere else.
    ///
    /// The recipient is a parameter only so the ChallengeManager can pass the
    /// treasury address it was initialized with; there is deliberately no path
    /// that lets a challenger, a victim or an operator name themselves here.
    /// Slashed stake must never be a prize, or manufacturing a true proof
    /// becomes a business model.
    ///
    /// The draw is capped by *this certificate's* allocation, so slashing
    /// certificate A cannot touch the allocation backing certificate B.
    pub fn slash_allocation(env: Env, cert_id: u64, treasury: Address, amount: i128) {
        let cm: Address = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeManager)
            .unwrap();
        cm.require_auth();

        if amount <= 0 {
            panic!("invalid_amount");
        }
        let allocation = Self::allocation_of(&env, cert_id);
        if amount > allocation {
            panic!("slash_exceeds_allocation");
        }

        let auditor: Address = env
            .storage()
            .persistent()
            .get(&DataKey::AllocationAuditor(cert_id))
            .expect("no_allocation");

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &treasury,
            &amount,
        );

        env.storage()
            .persistent()
            .set(&DataKey::Allocation(cert_id), &(allocation - amount));
        env.storage().persistent().set(
            &DataKey::Allocated(auditor.clone()),
            &(Self::allocated_of(&env, &auditor) - amount),
        );
        env.storage().persistent().set(
            &DataKey::Stake(auditor.clone()),
            &(Self::stake_of(&env, &auditor) - amount),
        );

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Allocation(cert_id));
        Self::bump(&env, &DataKey::AllocationAuditor(cert_id));
        Self::bump(&env, &DataKey::Allocated(auditor.clone()));
        Self::bump(&env, &DataKey::Stake(auditor));
    }

    /// Retire an allocation at settlement: whatever was not slashed goes back
    /// to the auditor's **free** stake.
    ///
    /// Without this the unslashed remainder would sit allocated to a dead
    /// certificate forever — capital stranded, which is the exact defect the
    /// per-certificate refactor exists to remove.
    pub fn retire_allocation(env: Env, cert_id: u64) {
        let cm: Address = env
            .storage()
            .instance()
            .get(&DataKey::ChallengeManager)
            .unwrap();
        cm.require_auth();
        Self::free_allocation(&env, cert_id);
    }

    /// The auditor frees a live allocation themselves, once the certificate's
    /// settlement deadline has passed and no challenge can still land on it.
    pub fn release_allocation(env: Env, cert_id: u64) {
        let auditor: Address = env
            .storage()
            .persistent()
            .get(&DataKey::AllocationAuditor(cert_id))
            .expect("no_allocation");
        auditor.require_auth();

        // The attest-time snapshot is a floor, not the answer. DESIGN-V2 §1
        // lets an open claim window push the certificate's settlement deadline
        // out past `expires_at + CHALLENGE_WINDOW`, and the snapshot taken at
        // attestation cannot know about a window opened later. The live
        // deadline is read from the Registry every time and the later of the
        // two wins, so the auditor cannot free the allocation that a live
        // window is about to settle against.
        let snapshot: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::AllocationUnlockAt(cert_id))
            .unwrap_or(0);
        let live = Self::cert_settlement_deadline(&env, cert_id);
        let unlock_at = if snapshot > live { snapshot } else { live };
        if env.ledger().timestamp() < unlock_at {
            panic!("allocation_still_locked");
        }

        Self::free_allocation(&env, cert_id);
    }

    /// Withdraw **free** stake. Allocated capital is untouchable by definition:
    /// it is locked because a live certificate stands on it, not because of a
    /// timestamp on the auditor.
    pub fn release(env: Env, auditor: Address) {
        auditor.require_auth();

        let free = Self::stake_of(&env, &auditor) - Self::allocated_of(&env, &auditor);
        if free <= 0 {
            return;
        }

        let token_addr: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        token::Client::new(&env, &token_addr).transfer(
            &env.current_contract_address(),
            &auditor,
            &free,
        );

        env.storage().persistent().set(
            &DataKey::Stake(auditor.clone()),
            &(Self::stake_of(&env, &auditor) - free),
        );

        Self::bump_instance(&env);
        Self::bump(&env, &DataKey::Stake(auditor.clone()));
        Self::bump_if_present(&env, &DataKey::Allocated(auditor));
    }

    // ----- internals -----

    /// Defect L2. Bump the instance entry (which carries the contract's own
    /// code reference) so the contract cannot archive out from under its users.
    fn bump_instance(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Defect L2. Bump one persistent entry.
    fn bump(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Defect L2. Bump a persistent entry that may not exist yet.
    ///
    /// `extend_ttl` on a missing key is a host error, not a no-op, and several
    /// of this contract's keys are genuinely absent until the auditor first
    /// allocates: `Allocated` and `LockedUntil` are only written by `allocate`.
    /// A blind bump would turn a plain `stake()` into a failed transaction.
    fn bump_if_present(env: &Env, key: &DataKey) {
        if env.storage().persistent().has(key) {
            Self::bump(env, key);
        }
    }

    fn stake_of(env: &Env, auditor: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Stake(auditor.clone()))
            .unwrap_or(0)
    }

    fn allocated_of(env: &Env, auditor: &Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Allocated(auditor.clone()))
            .unwrap_or(0)
    }

    /// The Registry owns the settlement deadline — `expires_at +
    /// CHALLENGE_WINDOW`, extended by any open claim window — so the reserve
    /// and this allocation can never drift apart.
    fn cert_settlement_deadline(env: &Env, cert_id: u64) -> u64 {
        let registry: Address = env.storage().instance().get(&DataKey::Registry).unwrap();
        env.invoke_contract(
            &registry,
            &Symbol::new(env, "get_cert_settlement_deadline"),
            Vec::from_array(env, [cert_id.into_val(env)]),
        )
    }

    fn allocation_of(env: &Env, cert_id: u64) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Allocation(cert_id))
            .unwrap_or(0)
    }

    fn free_allocation(env: &Env, cert_id: u64) {
        let remainder = Self::allocation_of(env, cert_id);
        let auditor: Option<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::AllocationAuditor(cert_id));
        let auditor = match auditor {
            None => return,
            Some(a) => a,
        };

        if remainder > 0 {
            env.storage().persistent().set(
                &DataKey::Allocated(auditor.clone()),
                &(Self::allocated_of(env, &auditor) - remainder),
            );
        }
        Self::bump_instance(env);
        Self::bump_if_present(env, &DataKey::Allocated(auditor.clone()));
        Self::bump_if_present(env, &DataKey::Stake(auditor));
        env.storage()
            .persistent()
            .remove(&DataKey::Allocation(cert_id));
        env.storage()
            .persistent()
            .remove(&DataKey::AllocationAuditor(cert_id));
        env.storage()
            .persistent()
            .remove(&DataKey::AllocationUnlockAt(cert_id));
    }
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
