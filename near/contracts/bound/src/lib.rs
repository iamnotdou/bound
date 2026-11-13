//! Bound on NEAR: one contract that holds the whole protocol.
//!
//! An operator publishes a certificate for its agent (a named NEAR account),
//! the agent accepts it, the operator funds a USDC reserve, and an auditor
//! attests by locking part of its stake against that one certificate. The
//! agent then spends only through `pay`, which meters every payment against
//! the operator-funded float, an optional recipient allowlist and a halt
//! switch. Anyone reads `verify(agent)` before dealing with the agent.
//!
//! If the agent causes harm, a claim opens a claim window that freezes the
//! certificate; `close_window` runs the settlement waterfall ported from the
//! Soroban `settle_fraud` (see `settle.rs`).
//!
//! Why ONE contract: NEAR cross-contract calls are asynchronous and do not
//! roll back the caller. Registry, reserve, staking, router and challenge
//! state live in one contract so settlement is one atomic state change. The
//! only outgoing promise is `withdraw` -> token `ft_transfer`, with a
//! re-crediting callback.
//!
//! Money model: tokens enter by NEP-141 `ft_transfer_call` and are credited to
//! an internal balance; everything inside the protocol moves internal
//! balances; tokens leave only by `withdraw` (pull, 1 yoctoNEAR).

use near_sdk::json_types::U128;
use near_sdk::serde_json::{self, json};
use near_sdk::store::{LookupMap, Vector};
use near_sdk::{
    AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseOrValue, env, near,
    require,
};

pub mod settle;
#[cfg(test)]
mod tests;

use settle::ClaimIn;

/// Storage deposit to register an internal balance (NEP-145 minimal).
pub const STORAGE_DEPOSIT: NearToken = NearToken::from_millinear(10); // 0.01 NEAR
/// Attached deposit required by `publish` (certificate, meter, window state).
pub const PUBLISH_DEPOSIT: NearToken = NearToken::from_millinear(50); // 0.05 NEAR
/// Attached deposit required by `challenge` (challenge record, window entry).
pub const CHALLENGE_DEPOSIT: NearToken = NearToken::from_millinear(10); // 0.01 NEAR
/// At most this many admitted claims per window; bounds `close_window` gas.
pub const MAX_CLAIMS_PER_WINDOW: usize = 16;
pub const MAX_ALLOWLIST: usize = 16;
pub const MAX_MEMO: usize = 64;
pub const MAX_EVIDENCE_URI: usize = 256;
pub const MAX_LIST_LIMIT: u64 = 50;

const GAS_FT_TRANSFER: Gas = Gas::from_tgas(10);
const GAS_ON_WITHDRAW: Gas = Gas::from_tgas(5);
const GAS_WITHDRAW_MIN: Gas = Gas::from_tgas(30);

#[derive(BorshStorageKey)]
#[near]
enum StorageKey {
    Balances,
    Stakes,
    Certs,
    AgentCert,
    Challenges,
    Windows,
}

/// Immutable protocol configuration, fixed at init. No setters exist.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug)]
pub struct Config {
    /// Length of a claim window, seconds (production 72h; testnet demo 90 s).
    pub claim_window_secs: u64,
    /// Seconds after `expires_at` during which challenges are still accepted
    /// and reserve/allocation stay locked (production 7d; testnet demo 300 s).
    pub post_expiry_secs: u64,
    /// Post-expiry spending inside this many seconds after expiry is not
    /// counted by the `ExpiredSpend` predicate (production 24h).
    pub grace_secs: u64,
    /// Minimum allocation an auditor may lock behind one certificate.
    pub min_stake: U128,
    /// Minimum challenge bond.
    pub min_challenge_bond: U128,
    /// Challenger fee as basis points of proven harm (1000 = 10%).
    pub challenger_fee_bps: u16,
    /// `ExpiredSpend` threshold as basis points of the bound (10 = 0.1%).
    pub de_minimis_bps: u16,
    /// Flat bounty for a hygiene-only window, paid from the reserve.
    pub hygiene_bounty: U128,
    /// Protocol share of the premium paid at attest, basis points (1000 = 10%).
    pub protocol_fee_bps: u16,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertStatus {
    /// Published by the operator, waiting for the agent.
    Pending,
    /// Agent accepted; waiting for reserve and attestation.
    Accepted,
    /// Attested by the auditor; the agent may pay.
    Verified,
    /// Settled against: no more spending, but still answers Harm claims
    /// and keeps what is left of its collateral until the deadline.
    Invalid,
    /// Collateral returned: dead.
    Released,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default)]
pub struct Settlement {
    pub mode: String,
    pub settled_at: u64,
    pub admitted: u32,
    pub payable: U128,
    pub reserve_draw: U128,
    /// Total paid to victims: from the reserve plus from the slash.
    pub victim_paid: U128,
    pub fee_paid: U128,
    pub slash: U128,
    /// Part of `slash` paid to victims (unpaid arbitrated harm first).
    pub slash_to_victims: U128,
    pub to_treasury: U128,
    /// The allocation left unslashed. It stays locked behind the certificate
    /// and returns to the auditor's free stake by `release_allocation` after
    /// the settlement deadline (the field name predates that rule).
    pub auditor_returned: U128,
    pub hygiene_bounty_paid: U128,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct Cert {
    pub id: u64,
    pub operator: AccountId,
    pub agent: AccountId,
    pub auditor: AccountId,
    pub bound: u128,
    pub reserve_promised: u128,
    pub reserve_locked: u128,
    pub float: u128,
    pub float_cap: u128,
    pub allowlist: Option<Vec<AccountId>>,
    pub expires_at: u64,
    pub premium: u128,
    pub allocation: u128,
    pub spent: u128,
    pub post_expiry_spent: u128,
    pub pay_count: u64,
    pub status: CertStatus,
    pub halted: bool,
    pub created_at: u64,
    pub attested_at: u64,
    pub settlement: Option<Settlement>,
}

/// Proof submitted with a challenge.
#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Proof {
    /// Trustless: locked reserve below the promised reserve.
    InsufficientReserve,
    /// Trustless hygiene: metered spend above the bound.
    BoundExceeded,
    /// Trustless hygiene: spend after expiry + grace above de minimis.
    ExpiredSpend,
    /// Arbiter-assessed harm (port of Soroban `FakeSignature`).
    Harm { claimed: U128, evidence_uri: String },
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Filed, inside an open window.
    Pending,
    /// Trustless predicate false at filing, or arbiter found it unproven:
    /// bond forfeited to the treasury.
    Rejected,
    /// Window closed with no arbiter decision: bond returned, no effect.
    Unadjudicated,
    /// Predicate no longer true at close: bond returned.
    Cured,
    /// Admitted and settled against the certificate: bond returned.
    Upheld,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct Challenge {
    pub id: u64,
    pub cert_id: u64,
    pub challenger: AccountId,
    pub victim: AccountId,
    pub proof: Proof,
    pub bond: u128,
    pub filed_at: u64,
    pub adjudicated: bool,
    pub proven: bool,
    pub arbitrated: bool,
    pub harm: u128,
    pub verdict: Verdict,
}

#[near(serializers = [borsh])]
#[derive(Clone, Debug)]
pub struct Window {
    pub opened_at: u64,
    pub closes_at: u64,
    pub claims: Vec<u64>,
}

#[near(serializers = [borsh, json])]
#[derive(Clone, Debug, Default)]
pub struct Stake {
    pub free: U128,
    pub allocated: U128,
}

/// What a counterparty reads before dealing with an agent.
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct CertView {
    pub cert_id: u64,
    /// Computed at read time: Pending, Accepted, Verified, Halted, Expired,
    /// Superseded, Challenged, Invalid, Released. Act only on "Verified".
    pub status: String,
    /// True only when status is "Verified" and the agent can pay right now
    /// through this certificate (not halted, float > 0). A counterparty that
    /// expects to be paid by the agent checks this.
    pub deal: bool,
    pub operator: AccountId,
    pub agent: AccountId,
    pub auditor: AccountId,
    pub bound: U128,
    pub reserve_promised: U128,
    pub reserve_locked: U128,
    pub allocation: U128,
    /// reserve_locked + allocation: the most that arbitrated victims of this
    /// certificate can recover in total. This, not `bound`, is the number a
    /// counterparty can rely on.
    pub coverage: U128,
    pub premium: U128,
    pub float: U128,
    pub float_cap: U128,
    pub spent: U128,
    pub post_expiry_spent: U128,
    pub pay_count: u64,
    pub allowlist: Option<Vec<AccountId>>,
    pub expires_at: u64,
    pub settlement_deadline: u64,
    pub halted: bool,
    pub window_closes_at: Option<u64>,
    pub open_claims: u32,
    pub settlement: Option<Settlement>,
    pub now: u64,
}

#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct ChallengeView {
    pub challenge_id: u64,
    pub cert_id: u64,
    pub challenger: AccountId,
    pub victim: AccountId,
    pub proof: Proof,
    pub bond: U128,
    pub filed_at: u64,
    pub adjudicated: bool,
    pub proven: bool,
    pub harm: U128,
    pub verdict: Verdict,
}

#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct WindowView {
    pub cert_id: u64,
    pub opened_at: u64,
    pub closes_at: u64,
    pub claims: Vec<u64>,
}

#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct StorageBalance {
    pub total: U128,
    pub available: U128,
}

#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct Accounts {
    pub owner: AccountId,
    pub token: AccountId,
    pub treasury: AccountId,
    pub arbiter: AccountId,
}

/// `msg` of an inbound `ft_transfer_call`. Empty msg credits the sender.
#[near(serializers = [json])]
#[derive(Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DepositMsg {
    /// Credit the internal balance of another registered account.
    Credit { account_id: AccountId },
    /// Credit the sender, then stake all of it (auditor).
    Stake {},
    /// Credit the sender, then fund that certificate's reserve (operator).
    Reserve { cert_id: u64 },
    /// Credit the sender, then fund that certificate's float (operator).
    Float { cert_id: u64 },
}

#[derive(PanicOnDefault)]
#[near(contract_state)]
pub struct Bound {
    owner: AccountId,
    token: AccountId,
    treasury: AccountId,
    arbiter: AccountId,
    config: Config,
    balances: LookupMap<AccountId, u128>,
    stakes: LookupMap<AccountId, Stake>,
    certs: Vector<Cert>,
    agent_cert: LookupMap<AccountId, u64>,
    challenges: Vector<Challenge>,
    windows: LookupMap<u64, Window>,
}

fn now_secs() -> u64 {
    env::block_timestamp() / 1_000_000_000
}

fn fail(code: &str, detail: impl AsRef<str>) -> ! {
    let d = detail.as_ref();
    if d.is_empty() {
        env::panic_str(code)
    } else {
        env::panic_str(&format!("{code}: {d}"))
    }
}

fn emit(event: &str, data: serde_json::Value) {
    let log = json!({
        "standard": "bound",
        "version": "1.0.0",
        "event": event,
        "data": [data],
    });
    env::log_str(&format!("EVENT_JSON:{log}"));
}

fn bps(amount: u128, bps: u16) -> u128 {
    amount.checked_mul(bps as u128).expect("overflow") / 10_000
}

fn assert_one_yocto() {
    require!(
        env::attached_deposit() == NearToken::from_yoctonear(1),
        "Requires attached deposit of exactly 1 yoctoNEAR"
    );
}

#[near]
impl Bound {
    /// One-shot init; deploy it in the same transaction as the code
    /// (deploy-with-init) so nobody can race to initialize.
    #[init]
    pub fn new(
        owner: AccountId,
        token: AccountId,
        treasury: AccountId,
        arbiter: AccountId,
        config: Config,
    ) -> Self {
        require!(
            config.challenger_fee_bps <= 10_000,
            "invalid challenger_fee_bps"
        );
        require!(
            config.protocol_fee_bps <= 10_000,
            "invalid protocol_fee_bps"
        );
        require!(config.de_minimis_bps <= 10_000, "invalid de_minimis_bps");
        require!(config.claim_window_secs > 0, "invalid claim_window_secs");
        require!(
            config.min_challenge_bond.0 > 0,
            "invalid min_challenge_bond"
        );
        let mut this = Self {
            owner,
            token,
            treasury: treasury.clone(),
            arbiter: arbiter.clone(),
            config,
            balances: LookupMap::new(StorageKey::Balances),
            stakes: LookupMap::new(StorageKey::Stakes),
            certs: Vector::new(StorageKey::Certs),
            agent_cert: LookupMap::new(StorageKey::AgentCert),
            challenges: Vector::new(StorageKey::Challenges),
            windows: LookupMap::new(StorageKey::Windows),
        };
        this.balances.insert(treasury, 0);
        this.balances.insert(arbiter, 0);
        this
    }

    // ------------------------------------------------------------------
    // Storage (minimal NEP-145)
    // ------------------------------------------------------------------

    /// Register an internal balance for `account_id` (default: caller) with a
    /// fixed 0.01 NEAR. Excess, or the whole deposit if already registered, is
    /// refunded. Deposits are not refundable in this version.
    #[payable]
    pub fn storage_deposit(&mut self, account_id: Option<AccountId>) -> StorageBalance {
        let account_id = account_id.unwrap_or_else(env::predecessor_account_id);
        let attached = env::attached_deposit();
        if self.balances.contains_key(&account_id) {
            if !attached.is_zero() {
                Promise::new(env::predecessor_account_id())
                    .transfer(attached)
                    .detach();
            }
        } else {
            require!(
                attached >= STORAGE_DEPOSIT,
                "StorageDepositTooSmall: requires 0.01 NEAR"
            );
            let before = env::storage_usage();
            self.balances.insert(account_id.clone(), 0);
            self.balances.flush();
            self.assert_storage_covered(before, STORAGE_DEPOSIT);
            let refund = attached.saturating_sub(STORAGE_DEPOSIT);
            if !refund.is_zero() {
                Promise::new(env::predecessor_account_id())
                    .transfer(refund)
                    .detach();
            }
            emit("register", json!({ "account_id": account_id }));
        }
        StorageBalance {
            total: U128(STORAGE_DEPOSIT.as_yoctonear()),
            available: U128(0),
        }
    }

    pub fn storage_balance_of(&self, account_id: AccountId) -> Option<StorageBalance> {
        self.balances.get(&account_id).map(|_| StorageBalance {
            total: U128(STORAGE_DEPOSIT.as_yoctonear()),
            available: U128(0),
        })
    }

    // ------------------------------------------------------------------
    // Money in and out
    // ------------------------------------------------------------------

    /// NEP-141 receiver. Only the configured token may call it. Returns the
    /// unused amount: the full amount when the beneficiary is not registered
    /// (the token refunds it), else 0. Any msg parse error or failed action
    /// panics, so the token refunds the whole transfer.
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        require!(
            env::predecessor_account_id() == self.token,
            "UnknownToken: only the configured token can deposit"
        );
        require!(amount.0 > 0, "InvalidAmount");
        let parsed: Option<DepositMsg> = if msg.trim().is_empty() {
            None
        } else {
            Some(serde_json::from_str(&msg).unwrap_or_else(|e| fail("InvalidMsg", e.to_string())))
        };
        let beneficiary = match &parsed {
            Some(DepositMsg::Credit { account_id }) => account_id.clone(),
            _ => sender_id.clone(),
        };
        if !self.balances.contains_key(&beneficiary) {
            // Never create an entry at the contract's expense.
            return PromiseOrValue::Value(amount);
        }
        self.credit(&beneficiary, amount.0);
        emit(
            "deposit",
            json!({ "account_id": beneficiary, "amount": amount, "sender_id": sender_id }),
        );
        match parsed {
            Some(DepositMsg::Stake {}) => self.internal_stake(&sender_id, amount.0),
            Some(DepositMsg::Reserve { cert_id }) => {
                self.internal_fund_reserve(&sender_id, cert_id, amount.0)
            }
            Some(DepositMsg::Float { cert_id }) => {
                self.internal_fund_float(&sender_id, cert_id, amount.0)
            }
            _ => {}
        }
        PromiseOrValue::Value(U128(0))
    }

    /// Pull withdrawal of the caller's internal balance to its token account.
    /// Debits first; the callback re-credits if the token transfer fails
    /// (e.g. the caller is not registered on the token).
    #[payable]
    pub fn withdraw(&mut self, amount: U128) -> Promise {
        assert_one_yocto();
        require!(
            env::prepaid_gas() >= GAS_WITHDRAW_MIN,
            "NotEnoughGas: withdraw needs at least 30 Tgas"
        );
        let account = env::predecessor_account_id();
        require!(amount.0 > 0, "InvalidAmount");
        self.debit(&account, amount.0);
        emit(
            "withdraw",
            json!({ "account_id": account, "amount": amount }),
        );
        let args = json!({ "receiver_id": account, "amount": amount, "memo": "bound withdraw" })
            .to_string()
            .into_bytes();
        Promise::new(self.token.clone())
            .function_call(
                "ft_transfer".to_string(),
                args,
                NearToken::from_yoctonear(1),
                GAS_FT_TRANSFER,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(GAS_ON_WITHDRAW)
                    .on_withdraw(account, amount),
            )
    }

    #[private]
    pub fn on_withdraw(&mut self, account_id: AccountId, amount: U128) -> bool {
        let ok = env::promise_result_checked(0, 64).is_ok();
        if !ok {
            self.credit(&account_id, amount.0);
            emit(
                "withdraw_failed",
                json!({ "account_id": account_id, "amount": amount, "recredited": true }),
            );
        }
        ok
    }

    // ------------------------------------------------------------------
    // Auditor staking
    // ------------------------------------------------------------------

    #[payable]
    pub fn stake(&mut self, amount: U128) {
        assert_one_yocto();
        self.internal_stake(&env::predecessor_account_id(), amount.0);
    }

    /// Move free stake (never allocated stake) back to the internal balance.
    #[payable]
    pub fn unstake(&mut self, amount: U128) {
        assert_one_yocto();
        let auditor = env::predecessor_account_id();
        require!(amount.0 > 0, "InvalidAmount");
        let mut s = self.stakes.get(&auditor).cloned().unwrap_or_default();
        if s.free.0 < amount.0 {
            fail(
                "InsufficientFreeStake",
                format!("free {} < {}", s.free.0, amount.0),
            );
        }
        s.free = U128(s.free.0 - amount.0);
        self.stakes.insert(auditor.clone(), s);
        self.credit(&auditor, amount.0);
        emit("unstake", json!({ "auditor": auditor, "amount": amount }));
    }

    // ------------------------------------------------------------------
    // Certificates
    // ------------------------------------------------------------------

    /// Operator publishes a certificate for `agent`. Requires 0.05 NEAR
    /// attached (state for the certificate, its meter and its window). An
    /// optional `premium` is escrowed from the operator's balance now and paid
    /// at attest (90% auditor, 10% treasury with the demo config).
    #[payable]
    #[allow(clippy::too_many_arguments)]
    pub fn publish(
        &mut self,
        agent: AccountId,
        bound: U128,
        reserve: U128,
        expires_at: u64,
        auditor: AccountId,
        float_cap: U128,
        allowlist: Option<Vec<AccountId>>,
        premium: Option<U128>,
    ) -> u64 {
        require!(
            env::attached_deposit() >= PUBLISH_DEPOSIT,
            "PublishDepositTooSmall: requires 0.05 NEAR"
        );
        let before = env::storage_usage();
        let operator = env::predecessor_account_id();
        self.assert_registered(&operator);
        let me = env::current_account_id();
        require!(
            agent != operator,
            "InvalidAgent: agent cannot be the operator"
        );
        require!(
            agent != me && auditor != me,
            "InvalidAccount: cannot be the contract"
        );
        require!(
            auditor != operator && auditor != agent,
            "InvalidAuditor: auditor must be independent of operator and agent"
        );
        require!(
            bound.0 > 0 && reserve.0 > 0,
            "InvalidAmount: bound and reserve must be > 0"
        );
        require!(
            float_cap.0 <= bound.0,
            "InvalidFloatCap: float_cap must be <= bound"
        );
        require!(
            expires_at > now_secs(),
            "InvalidExpiry: expires_at must be in the future"
        );
        if let Some(list) = &allowlist {
            require!(
                !list.is_empty() && list.len() <= MAX_ALLOWLIST,
                "InvalidAllowlist: 1 to 16 entries"
            );
            require!(
                !list.contains(&agent),
                "InvalidAllowlist: agent cannot be listed"
            );
        }
        let premium = premium.map(|p| p.0).unwrap_or(0);
        if premium > 0 {
            self.debit(&operator, premium);
        }
        let id = self.certs.len() as u64;
        let cert = Cert {
            id,
            operator: operator.clone(),
            agent: agent.clone(),
            auditor: auditor.clone(),
            bound: bound.0,
            reserve_promised: reserve.0,
            reserve_locked: 0,
            float: 0,
            float_cap: float_cap.0,
            allowlist,
            expires_at,
            premium,
            allocation: 0,
            spent: 0,
            post_expiry_spent: 0,
            pay_count: 0,
            status: CertStatus::Pending,
            halted: false,
            created_at: now_secs(),
            attested_at: 0,
            settlement: None,
        };
        self.certs.push(cert);
        self.certs.flush();
        // The deposit also prepays the agent index entry written at `accept`
        // and the window record written at the first claim.
        self.assert_storage_covered_with_reserve(before, PUBLISH_DEPOSIT, 400);
        emit(
            "publish",
            json!({ "cert_id": id, "operator": operator, "agent": agent, "auditor": auditor,
                    "bound": bound, "reserve": reserve, "expires_at": expires_at,
                    "float_cap": float_cap, "premium": U128(premium) }),
        );
        id
    }

    /// The agent consents to the certificate (replaces the Soroban dual
    /// signature). An agent can hold one live certificate at a time; a cert
    /// that is only Pending never blocks the agent, so nobody can grief an
    /// agent by publishing certificates naming it.
    #[payable]
    pub fn accept(&mut self, cert_id: u64) {
        assert_one_yocto();
        let mut c = self.cert(cert_id);
        let agent = env::predecessor_account_id();
        if agent != c.agent {
            fail("NotAgent", "only the certificate's agent can accept");
        }
        if c.status != CertStatus::Pending {
            fail("InvalidStatus", format!("{:?}", c.status));
        }
        require!(now_secs() < c.expires_at, "CertExpired");
        if let Some(prev) = self.agent_cert.get(&agent).copied()
            && prev != cert_id
            && self.is_live(&self.cert(prev))
        {
            fail("AgentHasLiveCert", format!("cert {prev} is still live"));
        }
        c.status = CertStatus::Accepted;
        self.put_cert(c);
        self.agent_cert.insert(agent.clone(), cert_id);
        emit("accept", json!({ "cert_id": cert_id, "agent": agent }));
    }

    /// Operator moves `amount` from its balance into the certificate reserve.
    #[payable]
    pub fn fund_reserve(&mut self, cert_id: u64, amount: U128) {
        assert_one_yocto();
        self.internal_fund_reserve(&env::predecessor_account_id(), cert_id, amount.0);
    }

    /// The named auditor locks `allocation` of its free stake behind this one
    /// certificate. Requires agent acceptance and a fully funded reserve.
    #[payable]
    pub fn attest(&mut self, cert_id: u64, allocation: U128) {
        assert_one_yocto();
        let mut c = self.cert(cert_id);
        let auditor = env::predecessor_account_id();
        if auditor != c.auditor {
            fail("NotAuditor", "only the named auditor can attest");
        }
        if c.status != CertStatus::Accepted {
            fail(
                "NotAccepted",
                format!("status {:?}; the agent must accept first", c.status),
            );
        }
        require!(now_secs() < c.expires_at, "CertExpired");
        if c.reserve_locked < c.reserve_promised {
            fail(
                "ReserveUnfunded",
                format!(
                    "locked {} < promised {}",
                    c.reserve_locked, c.reserve_promised
                ),
            );
        }
        if allocation.0 < self.config.min_stake.0 {
            fail(
                "AllocationBelowMinStake",
                format!("min {}", self.config.min_stake.0),
            );
        }
        let mut s = self.stakes.get(&auditor).cloned().unwrap_or_default();
        if s.free.0 < allocation.0 {
            fail(
                "InsufficientFreeStake",
                format!("free {} < {}", s.free.0, allocation.0),
            );
        }
        s.free = U128(s.free.0 - allocation.0);
        s.allocated = U128(s.allocated.0.checked_add(allocation.0).expect("overflow"));
        self.stakes.insert(auditor.clone(), s);
        // Flat premium (NICE N0): escrowed at publish, paid now.
        let premium = c.premium;
        let mut to_auditor = 0;
        let mut to_treasury = 0;
        if premium > 0 {
            to_treasury = bps(premium, self.config.protocol_fee_bps);
            to_auditor = premium - to_treasury;
            self.credit(&auditor, to_auditor);
            let t = self.treasury.clone();
            self.credit(&t, to_treasury);
            c.premium = 0;
        }
        c.allocation = allocation.0;
        c.status = CertStatus::Verified;
        c.attested_at = now_secs();
        self.put_cert(c);
        emit(
            "attest",
            json!({ "cert_id": cert_id, "auditor": auditor, "allocation": allocation,
                    "premium_to_auditor": U128(to_auditor), "premium_to_treasury": U128(to_treasury) }),
        );
    }

    /// After the settlement deadline (expires_at + post_expiry_secs) with no
    /// open window, or at any time before attestation: the operator takes
    /// back what is left of the reserve and the float. A settled (Invalid)
    /// certificate stays locked until the deadline too, so later victims can
    /// still be paid from what is left.
    #[payable]
    pub fn release_reserve(&mut self, cert_id: u64) -> U128 {
        assert_one_yocto();
        let mut c = self.cert(cert_id);
        if env::predecessor_account_id() != c.operator {
            fail("NotOperator", "");
        }
        if self.is_frozen(&c) {
            fail("CertFrozen", "a claim window is open");
        }
        let deadline = self.deadline(&c);
        let mut premium_refund = 0;
        match c.status {
            CertStatus::Pending | CertStatus::Accepted => {
                c.status = CertStatus::Released;
                // Unattested: refund an escrowed premium too.
                premium_refund = c.premium;
                c.premium = 0;
            }
            CertStatus::Verified | CertStatus::Invalid => {
                if now_secs() < deadline {
                    fail("Locked", format!("reserve locked until {deadline}"));
                }
                c.status = CertStatus::Released;
            }
            CertStatus::Released => {}
        }
        // An escrowed premium counts: a never-funded cert still refunds it.
        let out = c.reserve_locked + c.float + premium_refund;
        if out == 0 {
            fail("NothingToRelease", "");
        }
        c.reserve_locked = 0;
        c.float = 0;
        let op = c.operator.clone();
        self.credit(&op, out);
        self.put_cert(c);
        emit(
            "release_reserve",
            json!({ "cert_id": cert_id, "amount": U128(out) }),
        );
        U128(out)
    }

    /// After the settlement deadline with no open window, the auditor's
    /// allocation goes back to its free stake.
    #[payable]
    pub fn release_allocation(&mut self, cert_id: u64) -> U128 {
        assert_one_yocto();
        let mut c = self.cert(cert_id);
        if env::predecessor_account_id() != c.auditor {
            fail("NotAuditor", "");
        }
        if self.is_frozen(&c) {
            fail("CertFrozen", "a claim window is open");
        }
        let deadline = self.deadline(&c);
        if now_secs() < deadline {
            fail("Locked", format!("allocation locked until {deadline}"));
        }
        let a = c.allocation;
        if a == 0 {
            fail("NothingToRelease", "");
        }
        c.allocation = 0;
        let auditor = c.auditor.clone();
        let mut s = self.stakes.get(&auditor).cloned().unwrap_or_default();
        s.allocated = U128(s.allocated.0 - a);
        s.free = U128(s.free.0 + a);
        self.stakes.insert(auditor.clone(), s);
        self.put_cert(c);
        emit(
            "release_allocation",
            json!({ "cert_id": cert_id, "amount": U128(a) }),
        );
        U128(a)
    }

    // ------------------------------------------------------------------
    // Router: agent spending
    // ------------------------------------------------------------------

    #[payable]
    pub fn fund_float(&mut self, cert_id: u64, amount: U128) {
        assert_one_yocto();
        self.internal_fund_float(&env::predecessor_account_id(), cert_id, amount.0);
    }

    /// The agent pays `to` from the certificate float. Takes NO deposit, so a
    /// function-call access key scoped to this contract and method can call
    /// it. Every payment is metered and emits a NEP-297 `pay` event.
    pub fn pay(&mut self, cert_id: u64, to: AccountId, amount: U128, memo: Option<String>) {
        let mut c = self.cert(cert_id);
        let caller = env::predecessor_account_id();
        if caller != c.agent {
            fail(
                "NotAgent",
                format!("{caller} is not the agent of cert {cert_id}"),
            );
        }
        if c.status != CertStatus::Verified {
            fail("CertNotVerified", format!("status {:?}", c.status));
        }
        if self.is_frozen(&c) {
            fail(
                "CertFrozen",
                "a claim window is open; payments are suspended",
            );
        }
        if now_secs() >= self.deadline(&c) {
            fail(
                "SettlementDeadlinePassed",
                "no payments from the settlement deadline on (unchallengeable)",
            );
        }
        // Only the certificate `verify(agent)` reports can pay, so what a
        // counterparty reads is what meters the agent.
        if self.agent_cert.get(&c.agent).copied() != Some(c.id) {
            fail(
                "NotCurrentCert",
                "the agent accepted a newer certificate; this one cannot pay",
            );
        }
        if c.halted {
            fail("CertHalted", "the operator halted this certificate");
        }
        if amount.0 == 0 {
            fail("InvalidAmount", "");
        }
        if to == c.agent || to == env::current_account_id() {
            fail(
                "InvalidRecipient",
                "cannot pay the agent itself or the contract",
            );
        }
        if let Some(list) = &c.allowlist
            && !list.contains(&to)
        {
            fail(
                "RecipientNotAllowed",
                format!("{to} is not on the operator allowlist"),
            );
        }
        if !self.balances.contains_key(&to) {
            fail(
                "RecipientNotRegistered",
                format!("{to} has no storage_deposit on bound"),
            );
        }
        if amount.0 > c.float {
            fail(
                "FloatExceeded",
                format!("amount {} > float {}", amount.0, c.float),
            );
        }
        let memo = memo.unwrap_or_default();
        if memo.len() > MAX_MEMO {
            fail("MemoTooLong", "max 64 bytes");
        }
        let now = now_secs();
        c.float -= amount.0;
        c.spent = c.spent.checked_add(amount.0).expect("overflow");
        c.pay_count += 1;
        if now > c.expires_at.saturating_add(self.config.grace_secs) {
            c.post_expiry_spent = c.post_expiry_spent.checked_add(amount.0).expect("overflow");
        }
        let spent = c.spent;
        let agent = c.agent.clone();
        self.put_cert(c);
        self.credit(&to, amount.0);
        emit(
            "pay",
            json!({ "cert_id": cert_id, "agent": agent, "to": to, "amount": amount,
                    "memo": memo, "spent": U128(spent) }),
        );
    }

    #[payable]
    pub fn halt(&mut self, cert_id: u64) {
        assert_one_yocto();
        let mut c = self.operator_cert(cert_id);
        c.halted = true;
        self.put_cert(c);
        emit("halt", json!({ "cert_id": cert_id }));
    }

    #[payable]
    pub fn resume(&mut self, cert_id: u64) {
        assert_one_yocto();
        let mut c = self.operator_cert(cert_id);
        c.halted = false;
        self.put_cert(c);
        emit("resume", json!({ "cert_id": cert_id }));
    }

    /// Operator takes the unspent float back to its balance.
    #[payable]
    pub fn clawback(&mut self, cert_id: u64) -> U128 {
        assert_one_yocto();
        let mut c = self.operator_cert(cert_id);
        let f = c.float;
        c.float = 0;
        let op = c.operator.clone();
        self.put_cert(c);
        self.credit(&op, f);
        emit("clawback", json!({ "cert_id": cert_id, "amount": U128(f) }));
        U128(f)
    }

    // ------------------------------------------------------------------
    // Challenges and settlement
    // ------------------------------------------------------------------

    /// File a claim against a Verified certificate, posting `bond` from the
    /// caller's balance. Requires 0.01 NEAR attached. A trustless predicate
    /// that is false at filing is rejected in the same call and the bond goes
    /// to the treasury. Any other claim opens (or joins) the claim window and
    /// freezes the certificate.
    ///
    /// A settled (Invalid) certificate still accepts `Harm` claims until the
    /// settlement deadline: they are paid from what is left. New claims are
    /// accepted while `now < deadline`, or after it only into a window that
    /// opened before the deadline and is still open.
    #[payable]
    pub fn challenge(
        &mut self,
        cert_id: u64,
        proof: Proof,
        victim: Option<AccountId>,
        bond: U128,
    ) -> u64 {
        require!(
            env::attached_deposit() >= CHALLENGE_DEPOSIT,
            "ChallengeDepositTooSmall: requires 0.01 NEAR"
        );
        let before = env::storage_usage();
        let challenger = env::predecessor_account_id();
        let c = self.cert(cert_id);
        // From here on only the stored id is used: never the argument.
        let cert_id = c.id;
        let is_harm = matches!(proof, Proof::Harm { .. });
        match c.status {
            CertStatus::Verified => {}
            CertStatus::Invalid if is_harm => {}
            CertStatus::Invalid => fail(
                "NotChallengeable",
                "a settled certificate accepts only Harm claims",
            ),
            s => fail("NotChallengeable", format!("status {s:?}")),
        }
        if challenger == c.operator || challenger == c.agent || challenger == c.auditor {
            fail(
                "ConflictedChallenger",
                "the operator, agent and auditor cannot challenge their own certificate",
            );
        }
        let now = now_secs();
        let deadline = self.deadline(&c);
        let open_window = self.windows.get(&cert_id).cloned();
        // From the deadline on, the collateral is releasable: only a window
        // that opened before it, and is still open, takes more claims.
        let window_open = open_window.as_ref().is_some_and(|w| now < w.closes_at);
        if now >= deadline && !window_open {
            fail("SettlementDeadlinePassed", format!("deadline {deadline}"));
        }
        if bond.0 < self.config.min_challenge_bond.0 {
            fail(
                "BondTooSmall",
                format!("min {}", self.config.min_challenge_bond.0),
            );
        }
        let victim = victim.unwrap_or_else(|| challenger.clone());
        self.assert_registered(&victim);
        self.debit(&challenger, bond.0);

        let (adjudicated, proven, harm) = match &proof {
            Proof::InsufficientReserve => {
                let short = c.reserve_promised.saturating_sub(c.reserve_locked);
                (true, short > 0, short)
            }
            Proof::BoundExceeded => (true, c.spent > c.bound, 0),
            Proof::ExpiredSpend => (
                true,
                c.post_expiry_spent > bps(c.bound, self.config.de_minimis_bps),
                0,
            ),
            Proof::Harm {
                claimed,
                evidence_uri,
            } => {
                require!(claimed.0 > 0, "InvalidAmount: claimed harm must be > 0");
                require!(
                    evidence_uri.len() <= MAX_EVIDENCE_URI,
                    "EvidenceUriTooLong: max 256 bytes"
                );
                (false, false, 0)
            }
        };
        let id = self.challenges.len() as u64;
        let mut ch = Challenge {
            id,
            cert_id,
            challenger: challenger.clone(),
            victim: victim.clone(),
            proof: proof.clone(),
            bond: bond.0,
            filed_at: now,
            adjudicated,
            proven,
            arbitrated: false,
            harm,
            verdict: Verdict::Pending,
        };
        if adjudicated && !proven {
            // False trustless claim: rejected in the same transaction, bond
            // forfeited to the treasury.
            ch.verdict = Verdict::Rejected;
            let t = self.treasury.clone();
            self.credit(&t, bond.0);
            self.challenges.push(ch);
            self.challenges.flush();
            self.assert_storage_covered(before, CHALLENGE_DEPOSIT);
            emit(
                "challenge_rejected",
                json!({ "challenge_id": id, "cert_id": cert_id, "challenger": challenger,
                        "proof": proof, "bond_to_treasury": bond }),
            );
            return id;
        }
        let mut w = open_window.unwrap_or(Window {
            opened_at: now,
            closes_at: now + self.config.claim_window_secs,
            claims: vec![],
        });
        if w.claims.len() >= MAX_CLAIMS_PER_WINDOW {
            fail("WindowFull", "16 claims already filed in this window");
        }
        // A true trustless predicate settles the same way however many times
        // it is filed: one per kind per window, so they cannot squat slots.
        if !is_harm
            && w.claims.iter().any(|id| {
                self.challenges
                    .get(*id as u32)
                    .is_some_and(|x| x.proof == proof)
            })
        {
            fail(
                "DuplicateClaim",
                "this predicate is already filed in the window",
            );
        }
        // A claim filed late in a window extends it, so a late victim (and
        // the arbiter) always get at least a third of a window.
        let min_left = self.extension_secs();
        if w.closes_at < now.saturating_add(min_left) {
            w.closes_at = now.saturating_add(min_left);
        }
        w.claims.push(id);
        let closes_at = w.closes_at;
        self.windows.insert(cert_id, w);
        self.challenges.push(ch);
        self.challenges.flush();
        self.windows.flush();
        self.assert_storage_covered(before, CHALLENGE_DEPOSIT);
        emit(
            "challenge",
            json!({ "challenge_id": id, "cert_id": cert_id, "challenger": challenger,
                    "victim": victim, "proof": proof, "bond": bond, "window_closes_at": closes_at }),
        );
        id
    }

    /// The arbiter decides a `Harm` claim inside its open window.
    /// `harm <= claimed`. A rejected claim is settled at once: its bond goes
    /// to the treasury and its window slot is freed for another claim.
    #[payable]
    pub fn resolve_by_arbiter(&mut self, challenge_id: u64, proven: bool, harm: U128) {
        assert_one_yocto();
        if env::predecessor_account_id() != self.arbiter {
            fail("NotArbiter", "");
        }
        let idx = self.challenge_index(challenge_id);
        let mut ch = self
            .challenges
            .get(idx)
            .cloned()
            .unwrap_or_else(|| fail("NoSuchChallenge", challenge_id.to_string()));
        let claimed = match &ch.proof {
            Proof::Harm { claimed, .. } => claimed.0,
            _ => fail(
                "NotArbitrable",
                "only Harm claims are decided by the arbiter",
            ),
        };
        if ch.adjudicated || ch.verdict != Verdict::Pending {
            fail("AlreadyResolved", "");
        }
        if !self.windows.contains_key(&ch.cert_id) {
            fail("NoOpenWindow", "");
        }
        if proven {
            require!(
                harm.0 > 0 && harm.0 <= claimed,
                "InvalidHarm: 0 < harm <= claimed"
            );
        }
        ch.adjudicated = true;
        ch.proven = proven;
        ch.arbitrated = proven;
        ch.harm = if proven { harm.0 } else { 0 };
        let cert_id = ch.cert_id;
        let h = ch.harm;
        if !proven {
            let t = self.treasury.clone();
            self.credit(&t, ch.bond);
            ch.verdict = Verdict::Rejected;
            let mut w = self.windows.get(&cert_id).cloned().expect("window");
            w.claims.retain(|id| *id != challenge_id);
            self.windows.insert(cert_id, w);
        }
        self.challenges.replace(idx, ch);
        emit(
            "resolve",
            json!({ "challenge_id": challenge_id, "cert_id": cert_id, "proven": proven, "harm": U128(h) }),
        );
    }

    /// Anyone, once the window has closed: settle every claim in it in one
    /// atomic state change (port of Soroban `settle_window` / `settle_fraud` /
    /// `settle_hygiene`).
    #[payable]
    pub fn close_window(&mut self, cert_id: u64) -> Option<Settlement> {
        assert_one_yocto();
        let mut c = self.cert(cert_id);
        let cert_id = c.id;
        if !matches!(c.status, CertStatus::Verified | CertStatus::Invalid) {
            fail("NotChallengeable", format!("status {:?}", c.status));
        }
        let w = self
            .windows
            .get(&cert_id)
            .cloned()
            .unwrap_or_else(|| fail("NoOpenWindow", ""));
        let now = now_secs();
        if now < w.closes_at {
            fail(
                "WindowOpen",
                format!("closes at {} (now {now})", w.closes_at),
            );
        }
        // Arbiter grace: while a Harm claim is still undecided, nobody can
        // front-run the arbiter by closing right at `closes_at`.
        let grace_end = w.closes_at.saturating_add(self.extension_secs());
        if now < grace_end
            && w.claims.iter().any(|id| {
                let ch = self.challenges.get(*id as u32).expect("claim");
                !ch.adjudicated && matches!(ch.proof, Proof::Harm { .. })
            })
        {
            fail(
                "ArbiterGrace",
                format!("an undecided claim; closable at {grace_end} (now {now})"),
            );
        }
        let treasury = self.treasury.clone();

        // Classify claims.
        let mut admitted: Vec<Challenge> = vec![];
        let mut shortfall: u128 = 0;
        let mut shortfall_claims: u128 = 0;
        for id in &w.claims {
            let mut ch = self.challenges.get(*id as u32).cloned().expect("claim");
            if !ch.adjudicated {
                // Freezing a certificate is not free: half the bond of a claim
                // nobody decided goes to the treasury, half comes back.
                let who = ch.challenger.clone();
                let back = ch.bond / 2;
                self.credit(&who, back);
                self.credit(&treasury, ch.bond - back);
                ch.verdict = Verdict::Unadjudicated;
                self.challenges.replace(*id as u32, ch);
            } else if !ch.proven {
                // Trustless claims that were false never enter a window, and
                // arbiter-rejected claims leave it at `resolve_by_arbiter`.
                // Kept as a guard: never reached.
                self.credit(&treasury, ch.bond);
                ch.verdict = Verdict::Rejected;
                self.challenges.replace(*id as u32, ch);
            } else if !self.predicate_live(&c, &ch) {
                let who = ch.challenger.clone();
                self.credit(&who, ch.bond);
                ch.verdict = Verdict::Cured;
                self.challenges.replace(*id as u32, ch);
            } else {
                if !ch.arbitrated && ch.proof == Proof::InsufficientReserve {
                    shortfall_claims += 1;
                    shortfall = shortfall.max(ch.harm);
                }
                admitted.push(ch);
            }
        }
        self.windows.remove(&cert_id);

        if admitted.is_empty() {
            self.put_cert(c);
            emit(
                "window_closed",
                json!({ "cert_id": cert_id, "admitted": 0 }),
            );
            return None;
        }

        let claims: Vec<ClaimIn> = admitted
            .iter()
            .map(|ch| ClaimIn {
                weight: if ch.arbitrated {
                    ch.harm
                } else if ch.proof == Proof::InsufficientReserve {
                    shortfall / shortfall_claims
                } else {
                    0
                },
                arbitrated: ch.arbitrated,
            })
            .collect();
        let total_harm: u128 = claims.iter().map(|x| x.weight).sum();
        let auditor = c.auditor.clone();
        let mut stake = self.stakes.get(&auditor).cloned().unwrap_or_default();
        let allocation = c.allocation;
        let mut s = Settlement {
            settled_at: now,
            admitted: admitted.len() as u32,
            ..Default::default()
        };

        if total_harm > 0 {
            let p = settle::settle(
                c.reserve_locked,
                allocation,
                &claims,
                self.config.challenger_fee_bps as u128,
            );
            for (i, ch) in admitted.iter().enumerate() {
                let to_victim = p.victim_shares[i] + p.slash_shares[i];
                if to_victim > 0 {
                    self.credit(&ch.victim, to_victim);
                }
                if p.fee_shares[i] > 0 {
                    self.credit(&ch.challenger, p.fee_shares[i]);
                }
            }
            self.credit(&treasury, p.to_treasury());
            c.reserve_locked -= p.reserve_out();
            s.mode = "fraud".into();
            s.payable = U128(p.payable);
            s.reserve_draw = U128(p.reserve_draw);
            s.victim_paid = U128(p.victim_paid() + p.slash_to_victims());
            s.fee_paid = U128(p.fee_paid());
            s.slash = U128(p.slash);
            s.slash_to_victims = U128(p.slash_to_victims());
            s.to_treasury = U128(p.to_treasury());
            // The unslashed allocation stays locked behind this certificate
            // until the deadline (`release_allocation`).
            s.auditor_returned = U128(p.auditor_return);
            stake.allocated = U128(stake.allocated.0 - p.slash);
            c.allocation = p.auditor_return;
        } else {
            // Hygiene: covenant broken, nobody proven harmed. One flat bounty,
            // split equally, from the reserve. The allocation is not slashed
            // and stays locked until the deadline.
            let bounty = self.config.hygiene_bounty.0.min(c.reserve_locked);
            let share = bounty / admitted.len() as u128;
            for ch in &admitted {
                if share > 0 {
                    self.credit(&ch.challenger, share);
                }
            }
            let paid = share * admitted.len() as u128;
            c.reserve_locked -= paid;
            s.mode = "hygiene".into();
            s.hygiene_bounty_paid = U128(paid);
            s.auditor_returned = U128(allocation);
        }
        self.stakes.insert(auditor, stake);

        // Return every admitted bond and record the verdict.
        for ch in admitted {
            self.credit(&ch.challenger, ch.bond);
            let mut ch = ch;
            ch.verdict = Verdict::Upheld;
            let id = ch.id;
            self.challenges.replace(id as u32, ch);
        }

        // The certificate is dead for spending: halted, float back to the
        // operator. It still answers Harm claims until the deadline; what is
        // left of the reserve and allocation is released after it by
        // `release_reserve` / `release_allocation`.
        let f = c.float;
        c.float = 0;
        if f > 0 {
            let op = c.operator.clone();
            self.credit(&op, f);
        }
        c.halted = true;
        c.status = CertStatus::Invalid;
        c.settlement = Some(s.clone());
        self.put_cert(c);
        emit("settle", json!({ "cert_id": cert_id, "settlement": s }));
        Some(s)
    }

    // ------------------------------------------------------------------
    // Views
    // ------------------------------------------------------------------

    pub fn get_config(&self) -> Config {
        self.config.clone()
    }

    pub fn get_owner(&self) -> AccountId {
        self.owner.clone()
    }

    pub fn get_accounts(&self) -> Accounts {
        Accounts {
            owner: self.owner.clone(),
            token: self.token.clone(),
            treasury: self.treasury.clone(),
            arbiter: self.arbiter.clone(),
        }
    }

    pub fn balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.balances.get(&account_id).copied().unwrap_or(0))
    }

    pub fn get_stake(&self, auditor: AccountId) -> Stake {
        self.stakes.get(&auditor).cloned().unwrap_or_default()
    }

    /// The current certificate of `agent` (the last one it accepted), with
    /// status computed at read time. Act only on status "Verified".
    pub fn verify(&self, agent: AccountId) -> Option<CertView> {
        self.agent_cert
            .get(&agent)
            .and_then(|id| u32::try_from(*id).ok())
            .and_then(|i| self.certs.get(i))
            .map(|c| self.view(c))
    }

    pub fn get_cert(&self, cert_id: u64) -> Option<CertView> {
        u32::try_from(cert_id)
            .ok()
            .and_then(|i| self.certs.get(i))
            .map(|c| self.view(c))
    }

    pub fn list_certs(&self, from_index: Option<u64>, limit: Option<u64>) -> Vec<CertView> {
        let from = from_index.unwrap_or(0);
        let limit = limit.unwrap_or(MAX_LIST_LIMIT).min(MAX_LIST_LIMIT);
        let len = self.certs.len() as u64;
        (from..len.min(from.saturating_add(limit)))
            .filter_map(|i| self.certs.get(i as u32).map(|c| self.view(c)))
            .collect()
    }

    pub fn cert_count(&self) -> u64 {
        self.certs.len() as u64
    }

    pub fn challenge_count(&self) -> u64 {
        self.challenges.len() as u64
    }

    pub fn get_challenge(&self, challenge_id: u64) -> Option<ChallengeView> {
        u32::try_from(challenge_id)
            .ok()
            .and_then(|i| self.challenges.get(i))
            .map(|ch| ChallengeView {
                challenge_id: ch.id,
                cert_id: ch.cert_id,
                challenger: ch.challenger.clone(),
                victim: ch.victim.clone(),
                proof: ch.proof.clone(),
                bond: U128(ch.bond),
                filed_at: ch.filed_at,
                adjudicated: ch.adjudicated,
                proven: ch.proven,
                harm: U128(ch.harm),
                verdict: ch.verdict,
            })
    }

    pub fn get_window(&self, cert_id: u64) -> Option<WindowView> {
        u32::try_from(cert_id).ok()?;
        self.windows.get(&cert_id).map(|w| WindowView {
            cert_id,
            opened_at: w.opened_at,
            closes_at: w.closes_at,
            claims: w.claims.clone(),
        })
    }
}

// ----------------------------------------------------------------------
// Internals
// ----------------------------------------------------------------------

impl Bound {
    /// Load a certificate. The ABI id is a u64 but storage is indexed by u32:
    /// an id above u32::MAX is rejected, never truncated (a truncated id would
    /// alias another certificate and open a second, independent window).
    fn cert(&self, cert_id: u64) -> Cert {
        let c = u32::try_from(cert_id)
            .ok()
            .and_then(|i| self.certs.get(i))
            .cloned()
            .unwrap_or_else(|| fail("NoSuchCert", cert_id.to_string()));
        debug_assert_eq!(c.id, cert_id);
        c
    }

    fn challenge_index(&self, challenge_id: u64) -> u32 {
        u32::try_from(challenge_id)
            .ok()
            .filter(|i| *i < self.challenges.len())
            .unwrap_or_else(|| fail("NoSuchChallenge", challenge_id.to_string()))
    }

    /// Frozen = a claim window exists for this certificate's own id. Derived
    /// from storage, never from a flag that a second window could reset.
    fn is_frozen(&self, c: &Cert) -> bool {
        self.windows.contains_key(&c.id)
    }

    /// Late-claim extension and arbiter grace: a third of a claim window.
    fn extension_secs(&self) -> u64 {
        (self.config.claim_window_secs / 3).max(1)
    }

    fn put_cert(&mut self, c: Cert) {
        let id = c.id as u32;
        self.certs.replace(id, c);
    }

    fn operator_cert(&self, cert_id: u64) -> Cert {
        let c = self.cert(cert_id);
        if env::predecessor_account_id() != c.operator {
            fail("NotOperator", "");
        }
        c
    }

    fn deadline(&self, c: &Cert) -> u64 {
        c.expires_at.saturating_add(self.config.post_expiry_secs)
    }

    fn is_live(&self, c: &Cert) -> bool {
        matches!(c.status, CertStatus::Accepted | CertStatus::Verified)
            && (self.is_frozen(c) || now_secs() < c.expires_at)
    }

    fn predicate_live(&self, c: &Cert, ch: &Challenge) -> bool {
        match ch.proof {
            Proof::InsufficientReserve => c.reserve_locked < c.reserve_promised,
            Proof::BoundExceeded => c.spent > c.bound,
            Proof::ExpiredSpend => c.post_expiry_spent > bps(c.bound, self.config.de_minimis_bps),
            Proof::Harm { .. } => true,
        }
    }

    fn view(&self, c: &Cert) -> CertView {
        let now = now_secs();
        let w = self.windows.get(&c.id);
        let current = self.agent_cert.get(&c.agent).copied() == Some(c.id);
        let status = match c.status {
            CertStatus::Verified if self.is_frozen(c) => "Challenged",
            CertStatus::Verified if !current => "Superseded",
            CertStatus::Verified if now >= c.expires_at => "Expired",
            CertStatus::Verified if c.halted => "Halted",
            CertStatus::Pending => "Pending",
            CertStatus::Accepted => "Accepted",
            CertStatus::Verified => "Verified",
            CertStatus::Invalid => "Invalid",
            CertStatus::Released => "Released",
        };
        CertView {
            cert_id: c.id,
            status: status.to_string(),
            deal: status == "Verified" && c.float > 0,
            operator: c.operator.clone(),
            agent: c.agent.clone(),
            auditor: c.auditor.clone(),
            bound: U128(c.bound),
            reserve_promised: U128(c.reserve_promised),
            reserve_locked: U128(c.reserve_locked),
            allocation: U128(c.allocation),
            coverage: U128(c.reserve_locked + c.allocation),
            premium: U128(c.premium),
            float: U128(c.float),
            float_cap: U128(c.float_cap),
            spent: U128(c.spent),
            post_expiry_spent: U128(c.post_expiry_spent),
            pay_count: c.pay_count,
            allowlist: c.allowlist.clone(),
            expires_at: c.expires_at,
            settlement_deadline: self.deadline(c),
            halted: c.halted,
            window_closes_at: w.map(|w| w.closes_at),
            open_claims: w.map(|w| w.claims.len() as u32).unwrap_or(0),
            settlement: c.settlement.clone(),
            now,
        }
    }

    fn assert_registered(&self, account: &AccountId) {
        if !self.balances.contains_key(account) {
            fail(
                "NotRegistered",
                format!("{account} must call storage_deposit first"),
            );
        }
    }

    fn credit(&mut self, account: &AccountId, amount: u128) {
        if amount == 0 {
            return;
        }
        let bal = self
            .balances
            .get(account)
            .copied()
            .unwrap_or_else(|| fail("NotRegistered", account));
        self.balances
            .insert(account.clone(), bal.checked_add(amount).expect("overflow"));
    }

    fn debit(&mut self, account: &AccountId, amount: u128) {
        let bal = self
            .balances
            .get(account)
            .copied()
            .unwrap_or_else(|| fail("NotRegistered", account));
        if bal < amount {
            fail("InsufficientBalance", format!("balance {bal} < {amount}"));
        }
        self.balances.insert(account.clone(), bal - amount);
    }

    fn internal_stake(&mut self, auditor: &AccountId, amount: u128) {
        require!(amount > 0, "InvalidAmount");
        self.debit(auditor, amount);
        let mut s = self.stakes.get(auditor).cloned().unwrap_or_default();
        s.free = U128(s.free.0.checked_add(amount).expect("overflow"));
        self.stakes.insert(auditor.clone(), s);
        emit(
            "stake",
            json!({ "auditor": auditor, "amount": U128(amount) }),
        );
    }

    fn internal_fund_reserve(&mut self, operator: &AccountId, cert_id: u64, amount: u128) {
        require!(amount > 0, "InvalidAmount");
        let mut c = self.cert(cert_id);
        if *operator != c.operator {
            fail("NotOperator", "");
        }
        if !matches!(
            c.status,
            CertStatus::Pending | CertStatus::Accepted | CertStatus::Verified
        ) {
            fail("InvalidStatus", format!("{:?}", c.status));
        }
        self.debit(operator, amount);
        c.reserve_locked = c.reserve_locked.checked_add(amount).expect("overflow");
        let locked = c.reserve_locked;
        self.put_cert(c);
        emit(
            "fund_reserve",
            json!({ "cert_id": cert_id, "amount": U128(amount), "reserve_locked": U128(locked) }),
        );
    }

    fn internal_fund_float(&mut self, operator: &AccountId, cert_id: u64, amount: u128) {
        require!(amount > 0, "InvalidAmount");
        let mut c = self.cert(cert_id);
        if *operator != c.operator {
            fail("NotOperator", "");
        }
        if !matches!(c.status, CertStatus::Accepted | CertStatus::Verified) {
            fail("InvalidStatus", format!("{:?}", c.status));
        }
        let f = c.float.checked_add(amount).expect("overflow");
        if f > c.float_cap {
            fail(
                "FloatCapExceeded",
                format!("float {} > cap {}", f, c.float_cap),
            );
        }
        self.debit(operator, amount);
        c.float = f;
        self.put_cert(c);
        emit(
            "fund_float",
            json!({ "cert_id": cert_id, "amount": U128(amount), "float": U128(f) }),
        );
    }

    fn assert_storage_covered(&self, before: u64, deposit: NearToken) {
        self.assert_storage_covered_with_reserve(before, deposit, 0)
    }

    /// Panics unless the state written since `before` (plus `extra` bytes
    /// prepaid for later writes) costs no more than `deposit`.
    fn assert_storage_covered_with_reserve(&self, before: u64, deposit: NearToken, extra: u64) {
        let used = env::storage_usage().saturating_sub(before) + extra;
        let cost = env::storage_byte_cost().saturating_mul(used as u128);
        if cost > deposit {
            fail(
                "StorageNotCovered",
                format!("{used} bytes cost {} > deposit {}", cost, deposit),
            );
        }
    }
}
