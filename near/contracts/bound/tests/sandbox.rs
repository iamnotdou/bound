//! near-sandbox integration tests: both contracts compiled to wasm with
//! cargo-near, deployed to a local NEAR node, driven with real transactions.
//! Real NEP-141 `ft_transfer_call` deposits and real `withdraw` promises.
//!
//! Run: `cd near && cargo test` (downloads the sandbox binary on first run).

use near_api::{AccountId, NearGas, NearToken, NetworkConfig, Signer};
use near_sdk::serde_json::{Value, json};
use std::sync::Arc;

const U: u128 = 1_000_000;

struct Env {
    sandbox: near_sandbox::Sandbox,
    net: NetworkConfig,
    signer: Arc<Signer>,
    usdc: AccountId,
    bound: AccountId,
}

fn build(manifest: &str) -> Vec<u8> {
    let path = format!("{}/../{manifest}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
    let opts = cargo_near_build::BuildOpts::builder()
        .manifest_path(path)
        .build();
    let artifact = cargo_near_build::build_with_cli(opts).expect("cargo near build");
    std::fs::read(artifact).expect("read wasm")
}

async fn account(sandbox: &near_sandbox::Sandbox, name: &str) -> AccountId {
    let id: AccountId = format!("{name}.sandbox").parse().unwrap();
    sandbox
        .create_account(id.as_str().parse().unwrap())
        .initial_balance(NearToken::from_near(20))
        .send()
        .await
        .expect("create account");
    id
}

impl Env {
    async fn start(claim_window_secs: u64) -> Env {
        let usdc_wasm = build("mock-usdc");
        let bound_wasm = build("bound");
        let sandbox = near_sandbox::Sandbox::start_sandbox()
            .await
            .expect("sandbox");
        let net = NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse().unwrap());
        let signer = Signer::from_secret_key(
            near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
                .parse()
                .unwrap(),
        )
        .unwrap();
        let usdc = account(&sandbox, "usdc").await;
        let bound = account(&sandbox, "bound").await;
        for n in [
            "treasury",
            "arbiter",
            "operator",
            "agent",
            "auditor",
            "service",
            "challenger",
            "nofttoken",
        ] {
            account(&sandbox, n).await;
        }
        near_api::Contract::deploy(usdc.clone())
            .use_code(usdc_wasm)
            .with_init_call("new", json!({}))
            .unwrap()
            .with_signer(signer.clone())
            .send_to(&net)
            .await
            .unwrap()
            .assert_success();
        near_api::Contract::deploy(bound.clone())
            .use_code(bound_wasm)
            .with_init_call(
                "new",
                json!({
                    "owner": "operator.sandbox",
                    "token": usdc,
                    "treasury": "treasury.sandbox",
                    "arbiter": "arbiter.sandbox",
                    "config": {
                        "claim_window_secs": claim_window_secs,
                        "post_expiry_secs": 300,
                        "grace_secs": 60,
                        "min_stake": (10 * U).to_string(),
                        "min_challenge_bond": (10 * U).to_string(),
                        "challenger_fee_bps": 1000,
                        "de_minimis_bps": 10,
                        "hygiene_bounty": (10 * U).to_string(),
                        "protocol_fee_bps": 1000
                    }
                }),
            )
            .unwrap()
            .with_signer(signer.clone())
            .send_to(&net)
            .await
            .unwrap()
            .assert_success();
        // The bound contract must hold a token account to receive deposits.
        near_api::Contract(usdc.clone())
            .call_function("storage_deposit", json!({ "account_id": bound }))
            .transaction()
            .deposit(NearToken::from_millinear(5))
            .with_signer("operator.sandbox".parse().unwrap(), signer.clone())
            .send_to(&net)
            .await
            .unwrap()
            .assert_success();
        Env {
            sandbox,
            net,
            signer,
            usdc,
            bound,
        }
    }

    fn id(name: &str) -> AccountId {
        format!("{name}.sandbox").parse().unwrap()
    }

    async fn call(
        &self,
        who: &str,
        contract: &AccountId,
        method: &str,
        args: Value,
        deposit: NearToken,
    ) -> near_api::types::transaction::result::TransactionResult {
        near_api::Contract(contract.clone())
            .call_function(method, args)
            .transaction()
            .deposit(deposit)
            .gas(NearGas::from_tgas(200))
            .with_signer(Self::id(who), self.signer.clone())
            .send_to(&self.net)
            .await
            .expect("send")
    }

    async fn ok(&self, who: &str, contract: &AccountId, method: &str, args: Value, d: NearToken) {
        let r = self.call(who, contract, method, args, d).await;
        r.assert_success();
    }

    async fn bound_ok(&self, who: &str, method: &str, args: Value, d: NearToken) {
        let c = self.bound.clone();
        self.ok(who, &c, method, args, d).await
    }

    async fn bound_fails(&self, who: &str, method: &str, args: Value, d: NearToken, expect: &str) {
        let c = self.bound.clone();
        let r = self.call(who, &c, method, args, d).await;
        let msg = format!("{:?}", r.assert_failure());
        assert!(msg.contains(expect), "expected {expect}, got {msg}");
    }

    async fn view<T: near_sdk::serde::de::DeserializeOwned + Send + Sync>(
        &self,
        contract: &AccountId,
        method: &str,
        args: Value,
    ) -> T {
        near_api::Contract(contract.clone())
            .call_function(method, args)
            .read_only()
            .fetch_from(&self.net)
            .await
            .expect("view")
            .data
    }

    async fn bound_bal(&self, who: &str) -> u128 {
        let v: String = self
            .view(
                &self.bound,
                "balance_of",
                json!({ "account_id": Self::id(who) }),
            )
            .await;
        v.parse().unwrap()
    }

    async fn ft_bal(&self, who: &str) -> u128 {
        let v: String = self
            .view(
                &self.usdc,
                "ft_balance_of",
                json!({ "account_id": Self::id(who) }),
            )
            .await;
        v.parse().unwrap()
    }

    async fn mint(&self, who: &str, amount: u128) {
        let usdc = self.usdc.clone();
        self.ok(
            "operator",
            &usdc,
            "mint",
            json!({ "account_id": Self::id(who), "amount": amount.to_string() }),
            NearToken::from_yoctonear(0),
        )
        .await;
    }

    async fn ft_transfer_call(&self, who: &str, amount: u128, msg: &str) {
        let usdc = self.usdc.clone();
        self.ok(
            who,
            &usdc,
            "ft_transfer_call",
            json!({ "receiver_id": self.bound, "amount": amount.to_string(), "msg": msg }),
            NearToken::from_yoctonear(1),
        )
        .await;
    }

    async fn register(&self, who: &str) {
        self.bound_ok(
            who,
            "storage_deposit",
            json!({}),
            NearToken::from_millinear(10),
        )
        .await;
    }

    async fn now(&self) -> u64 {
        let v: Value = self
            .view(&self.bound, "get_cert", json!({ "cert_id": 0 }))
            .await;
        v["now"].as_u64().unwrap()
    }

    async fn verify(&self, agent: &str) -> Value {
        self.view(&self.bound, "verify", json!({ "agent": Self::id(agent) }))
            .await
    }

    /// Demo actors funded, the demo certificate published, accepted, funded
    /// and attested. Returns nothing; cert id is 0.
    async fn verified_cert(&self) {
        self.mint("operator", 1_000 * U).await;
        self.mint("auditor", 500 * U).await;
        self.mint("service", 10 * U).await;
        self.mint("challenger", 50 * U).await;
        for who in ["operator", "auditor", "service", "challenger"] {
            self.register(who).await;
        }
        self.ft_transfer_call("auditor", 500 * U, "{\"stake\":{}}")
            .await;
        self.ft_transfer_call("operator", 200 * U, "").await;
        self.ft_transfer_call("service", 10 * U, "").await;
        self.ft_transfer_call("challenger", 50 * U, "").await;

        let expires_at = self.sandbox_now().await + 3600;
        self.bound_ok(
            "operator",
            "publish",
            json!({
                "agent": Self::id("agent"), "bound": (150 * U).to_string(),
                "reserve": (100 * U).to_string(), "expires_at": expires_at,
                "auditor": Self::id("auditor"), "float_cap": (20 * U).to_string(),
                "allowlist": [Self::id("service")], "premium": (3 * U).to_string()
            }),
            NearToken::from_millinear(50),
        )
        .await;
        let y = NearToken::from_yoctonear(1);
        self.bound_ok("agent", "accept", json!({ "cert_id": 0 }), y)
            .await;
        self.bound_ok(
            "operator",
            "fund_reserve",
            json!({ "cert_id": 0, "amount": (100 * U).to_string() }),
            y,
        )
        .await;
        // float through ft_transfer_call with the `float` action
        self.ft_transfer_call("operator", 20 * U, "{\"float\":{\"cert_id\":0}}")
            .await;
        self.bound_ok(
            "auditor",
            "attest",
            json!({ "cert_id": 0, "allocation": (200 * U).to_string() }),
            y,
        )
        .await;
    }

    /// Chain time in seconds from the latest block.
    async fn sandbox_now(&self) -> u64 {
        let block = near_api::Chain::block()
            .fetch_from(&self.net)
            .await
            .expect("block");
        block.header.timestamp_nanosec.parse::<u64>().expect("ts") / 1_000_000_000
    }

    async fn wait_past(&self, ts: u64) {
        for _ in 0..40 {
            if self.sandbox_now().await > ts {
                return;
            }
            self.sandbox.fast_forward(100).await.expect("fast forward");
        }
        panic!("chain time never passed {ts}");
    }
}

#[tokio::test]
async fn full_lifecycle_settles_victim() -> testresult::TestResult<()> {
    let e = Env::start(90).await;
    e.verified_cert().await;
    let y = NearToken::from_yoctonear(1);
    let zero = NearToken::from_yoctonear(0);

    let v = e.verify("agent").await;
    assert_eq!(v["status"], "Verified");
    assert_eq!(v["reserve_locked"], (100 * U).to_string());
    assert_eq!(v["float"], (20 * U).to_string());
    // premium 3: auditor 2.7, treasury 0.3
    assert_eq!(e.bound_bal("auditor").await, 2_700_000);
    assert_eq!(e.bound_bal("treasury").await, 300_000);

    // The agent pays the service inside the float: metered, event emitted.
    let r = e
        .call(
            "agent",
            &e.bound,
            "pay",
            json!({ "cert_id": 0, "to": Env::id("service"), "amount": (5 * U).to_string(), "memo": "api call 1" }),
            zero,
        )
        .await;
    let logs: Vec<String> = r.logs().iter().map(|s| s.to_string()).collect();
    r.assert_success();
    assert!(
        logs.iter()
            .any(|l| l.contains("EVENT_JSON") && l.contains("\"event\":\"pay\"")),
        "pay event in {logs:?}"
    );

    // Attacks, rejected on chain.
    e.bound_fails(
        "agent",
        "pay",
        json!({ "cert_id": 0, "to": Env::id("service"), "amount": (50 * U).to_string() }),
        zero,
        "FloatExceeded",
    )
    .await;
    e.bound_fails(
        "agent",
        "pay",
        json!({ "cert_id": 0, "to": Env::id("challenger"), "amount": (10 * U).to_string() }),
        zero,
        "RecipientNotAllowed",
    )
    .await;
    e.bound_fails(
        "operator",
        "pay",
        json!({ "cert_id": 0, "to": Env::id("service"), "amount": U.to_string() }),
        zero,
        "NotAgent",
    )
    .await;
    e.bound_ok("operator", "halt", json!({ "cert_id": 0 }), y)
        .await;
    e.bound_fails(
        "agent",
        "pay",
        json!({ "cert_id": 0, "to": Env::id("service"), "amount": U.to_string() }),
        zero,
        "CertHalted",
    )
    .await;

    // A false trustless claim (spent <= bound) is rejected in the same tx;
    // bond to treasury.
    e.bound_ok(
        "challenger",
        "challenge",
        json!({ "cert_id": 0, "proof": "BoundExceeded", "bond": (10 * U).to_string() }),
        NearToken::from_millinear(10),
    )
    .await;
    let ch: Value = e
        .view(&e.bound, "get_challenge", json!({ "challenge_id": 0 }))
        .await;
    assert_eq!(ch["verdict"], "Rejected");
    assert_eq!(e.bound_bal("treasury").await, 300_000 + 10 * U);

    // The service files Harm 120; the arbiter upholds it.
    e.bound_ok(
        "service",
        "challenge",
        json!({ "cert_id": 0, "proof": { "Harm": { "claimed": (120 * U).to_string(),
                "evidence_uri": "gate-invoices.json#inv-1" } }, "bond": (10 * U).to_string() }),
        NearToken::from_millinear(10),
    )
    .await;
    e.bound_ok(
        "arbiter",
        "resolve_by_arbiter",
        json!({ "challenge_id": 1, "proven": true, "harm": (120 * U).to_string() }),
        y,
    )
    .await;
    assert_eq!(e.verify("agent").await["status"], "Challenged");
    e.bound_ok("operator", "resume", json!({ "cert_id": 0 }), y)
        .await;
    e.bound_fails(
        "agent",
        "pay",
        json!({ "cert_id": 0, "to": Env::id("service"), "amount": U.to_string() }),
        zero,
        "CertFrozen",
    )
    .await;
    e.bound_fails(
        "operator",
        "close_window",
        json!({ "cert_id": 0 }),
        y,
        "WindowOpen",
    )
    .await;

    // Wait out the window on chain time, then anyone closes it.
    let w: Value = e
        .view(&e.bound, "get_window", json!({ "cert_id": 0 }))
        .await;
    e.wait_past(w["closes_at"].as_u64().unwrap()).await;
    e.bound_ok("challenger", "close_window", json!({ "cert_id": 0 }), y)
        .await;

    let v = e.verify("agent").await;
    assert_eq!(v["status"], "Invalid");
    let s = &v["settlement"];
    assert_eq!(s["mode"], "fraud");
    assert_eq!(s["payable"], (120 * U).to_string());
    // 100 from the reserve + 20 of the auditor's slash: the victim is whole.
    assert_eq!(s["victim_paid"], (120 * U).to_string());
    assert_eq!(s["slash"], (20 * U).to_string());
    assert_eq!(s["slash_to_victims"], (20 * U).to_string());
    assert_eq!(s["auditor_returned"], (180 * U).to_string());
    assert_eq!(e.bound_bal("service").await, 135 * U);
    assert_eq!(e.bound_bal("treasury").await, 300_000 + 10 * U);
    let stake: Value = e
        .view(
            &e.bound,
            "get_stake",
            json!({ "auditor": Env::id("auditor") }),
        )
        .await;
    assert_eq!(stake["free"], (480 * U).to_string());
    assert_eq!(stake["allocated"], "0");

    // The victim pulls real tokens out.
    let before = e.ft_bal("service").await;
    e.bound_ok(
        "service",
        "withdraw",
        json!({ "amount": (135 * U).to_string() }),
        y,
    )
    .await;
    assert_eq!(e.ft_bal("service").await, before + 135 * U);
    assert_eq!(e.bound_bal("service").await, 0);
    let _ = e.now().await;
    Ok(())
}

#[tokio::test]
async fn withdraw_failure_recredits_and_guards_hold() -> testresult::TestResult<()> {
    let e = Env::start(90).await;
    e.mint("operator", 10 * U).await;
    e.register("operator").await;
    e.register("nofttoken").await;
    // credit an account that is registered on bound but NOT on the token
    e.ft_transfer_call(
        "operator",
        3 * U,
        "{\"credit\":{\"account_id\":\"nofttoken.sandbox\"}}",
    )
    .await;
    assert_eq!(e.bound_bal("nofttoken").await, 3 * U);
    // withdraw: ft_transfer fails (receiver not registered on token), the
    // callback re-credits, so nothing is lost.
    e.bound_ok(
        "nofttoken",
        "withdraw",
        json!({ "amount": (3 * U).to_string() }),
        NearToken::from_yoctonear(1),
    )
    .await;
    assert_eq!(e.bound_bal("nofttoken").await, 3 * U);
    assert_eq!(e.ft_bal("nofttoken").await, 0);

    // unregistered beneficiary: the token refunds the sender in full
    let before = e.ft_bal("operator").await;
    e.ft_transfer_call(
        "operator",
        2 * U,
        "{\"credit\":{\"account_id\":\"agent.sandbox\"}}",
    )
    .await;
    assert_eq!(e.ft_bal("operator").await, before);

    // a bad msg: the token refunds the sender in full
    e.ft_transfer_call("operator", 2 * U, "{garbage").await;
    assert_eq!(e.ft_bal("operator").await, before);

    // only the token may call ft_on_transfer
    e.bound_fails(
        "operator",
        "ft_on_transfer",
        json!({ "sender_id": "operator.sandbox", "amount": "1", "msg": "" }),
        NearToken::from_yoctonear(0),
        "UnknownToken",
    )
    .await;
    // 1 yocto guard on withdraw
    e.bound_fails(
        "operator",
        "withdraw",
        json!({ "amount": "1" }),
        NearToken::from_yoctonear(0),
        "exactly 1 yoctoNEAR",
    )
    .await;
    // a second init is refused
    e.bound_fails(
        "operator",
        "new",
        json!({ "owner": "operator.sandbox", "token": "usdc.sandbox",
                "treasury": "treasury.sandbox", "arbiter": "arbiter.sandbox",
                "config": { "claim_window_secs": 1, "post_expiry_secs": 1, "grace_secs": 1,
                            "min_stake": "1", "min_challenge_bond": "1", "challenger_fee_bps": 0,
                            "de_minimis_bps": 0, "hygiene_bounty": "0", "protocol_fee_bps": 0 } }),
        NearToken::from_yoctonear(0),
        "already been initialized",
    )
    .await;
    Ok(())
}
