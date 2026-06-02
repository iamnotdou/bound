# CCP Stellar — Resources & Technical Reference

Everything you need to build. Do not guess URLs — use these exact links.

---

## Hackathon Info

- **Event:** Build On Stellar — IBW 2026 Istanbul (36 hours)
- **Host:** Rise In + Stellar Development Foundation
- **Venue:** Hilton Istanbul Bomonti Hotel & Conference Center, Şişli
- **Submission portal:** https://www.risein.com/programs/build-on-stellar-hackathon
- **Presentation template:** https://docs.google.com/presentation/d/1oRWx77PH3WsQ67Is9Xhg3GVNmkwVO7lU4Ab241__ahE/edit?usp=sharing

### Prize Pool

| Track | 1st | 2nd | 3rd | Total |
|---|---|---|---|---|
| Main Track | $1,200 | $800 | $600 | $2,600 |
| Hack Agentic | $700 | $500 | — | $1,200 |
| Hack Privacy | $700 | $500 | — | $1,200 |

**Target: Main + Hack Agentic = $3,800 max**

---

## Stellar Core Infrastructure

### Getting Started
- Smart contract setup: https://developers.stellar.org/docs/smart-contracts/getting-started/setup
- Getting started guide: https://developers.stellar.org/docs/build/smart-contracts/getting-started/hello-world-frontend

### Network Access
- Stellar RPC (real-time data): https://developers.stellar.org/docs/data/apis/rpc
- Stellar Lab (interact + query): https://lab.stellar.org
- Stellar.Expert (block explorer): https://stellar.expert
- Testnet Horizon: https://horizon-testnet.stellar.org
- Testnet Soroban RPC: https://soroban-testnet.stellar.org

### Fund Testnet Accounts
- Testnet faucet: https://lab.stellar.org/account/fund?$=network$id=testnet&label=Testnet&horizonUrl=https:////horizon-testnet.stellar.org&rpcUrl=https:////soroban-testnet.stellar.org&passphrase=Test%20SDF%20Network%20/;%20September%202015
- Circle USDC + EURC testnet faucet: https://faucet.circle.com

---

## Smart Contract Development (Soroban / Rust)

### Documentation
- Example contracts (DeFi, tokens, etc.): https://developers.stellar.org/docs/build/smart-contracts/example-contracts
- Smart contract authorization: https://developers.stellar.org/docs/learn/encyclopedia/security/authorization
- Developer tools list: https://developers.stellar.org/docs/tools/developer-tools

### SDK Library
- All language SDKs: https://developers.stellar.org/docs/tools/sdks/library

### Install Stellar CLI
```bash
cargo install --locked stellar-cli
```

### Init Soroban workspace
```bash
stellar contract init ccp-stellar
cd ccp-stellar
```

### Deploy a contract
```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/contract_name.wasm \
  --source <your-secret-key> \
  --network testnet
```

### Call a contract
```bash
stellar contract invoke \
  --id <contract-address> \
  --source <your-secret-key> \
  --network testnet \
  -- function_name --arg1 value1
```

### Generate TypeScript bindings (auto-generated client)
```bash
stellar contract bindings typescript \
  --network testnet \
  --contract-id <contract-address> \
  --output-dir ./sdk/src/contracts/<contract-name>
```

---

## Agentic Payments (x402)

This is the key Stellar technology for CCP. x402 is Stellar's native protocol for AI agent payments.

### Documentation
- Agentic payments overview: https://developers.stellar.org/docs/build/agentic-payments
- x402 protocol: https://developers.stellar.org/docs/build/agentic-payments/x402
- x402 built on Stellar: https://developers.stellar.org/docs/build/agentic-payments/x402/built-on-stellar
- x402 quickstart guide: https://developers.stellar.org/docs/build/agentic-payments/x402/quickstart-guide

### How x402 works with CCP
```
Agent → HTTP GET /some-service
Server → 402 Payment Required (x402 header with payment details)
Agent → calls SpendingLimit.pay(amount, server_address) via x402
  → if amount ≤ hard_limit: payment executes, agent gets service
  → if amount > hard_limit: REVERT, agent cannot proceed
Server → receives payment → serves response
```

---

## Smart Wallets & Passkeys

### Passkey-Kit (recommended for operator/auditor UX)
- GitHub: https://github.com/kalepail/passkey-kit
- Description: TypeScript SDK for creating and managing Stellar smart wallets
- Use for: operator wallet, auditor wallet onboarding

### Smart Wallet Docs
- https://developers.stellar.org/docs/build/apps/smart-wallets

### Demo Apps (reference implementations)
- Svelte/Astro flavor: https://github.com/kalepail/smart-stellar-demo
- React flavor: https://github.com/carstenjacobsen/smart-stellar-demo
- Vanilla flavor: https://github.com/elliotfriend/snapchain-demo

---

## Frontend

### Templates (pick one and fork)
- Scaffold Stellar Astro Template: https://github.com/AhaLabs/scaffold-stellar-frontend
- SvelteKit + Passkey Kit Template: https://github.com/ElliotFriend/soroban-template-sveltekit-passkeys

### Stellar Design System (React components)
- https://design-system.stellar.org

### Tutorial
- Guestbook dapp with passkey wallets: https://developers.stellar.org/docs/build/apps/guestbook/overview
- Build applications tutorials: https://developers.stellar.org/docs/building-apps/overview

---

## Stablecoin Resources

- Circle USDC/EURC on Stellar: native, no bridging needed
- Testnet faucet: https://faucet.circle.com
- OpenZeppelin Relayer docs: https://docs.openzeppelin.com/relayer
- OpenZeppelin open source tools: https://docs.openzeppelin.com/open-source-tools
- a16z stablecoins essay: https://a16zcrypto.com/posts/article/how-stablecoins-will-eat-payments

---

## Oracles (if price feeds needed)

- Reflector Network: https://reflector.network
- Oracle docs: https://developers.stellar.org/docs/data/oracles
- Noeracle: https://noeracle.org (docs: docs.noeracle.org)

---

## Privacy / ZK Resources (not our track, but for reference)

- Privacy guide: https://developers.stellar.org/docs/build/apps/privacy
- ZK guide: https://developers.stellar.org/docs/build/apps/zk
- Drand Relay: https://github.com/kaankacar/Drand-Relay
- Nethermind private payments: https://github.com/NethermindEth/stellar-private-payments

---

## Ecosystem Resources

- Stellar Ecosystem Resources (GitHub): https://github.com/stellar/ecosystem-resources
- Stellar AI Skills: https://skills.stellar.org
- Workshop reference project (vibe coding): https://github.com/kaankacar/stellar-build
- Workshop ZK project: https://github.com/atahanyild/stellar-privacy-workshop

---

## OpenZeppelin Relayer

- Docs: https://docs.openzeppelin.com/relayer
- Can be used to relay transactions on behalf of users (gasless UX)

---

## Monorepo Setup

### Recommended stack
- Rust + Soroban for contracts
- TypeScript + `@stellar/stellar-sdk` for SDK + scripts
- Next.js for web app
- pnpm workspaces + turborepo

### package.json (root)
```json
{
  "name": "ccp-stellar",
  "private": true,
  "workspaces": ["apps/*", "sdk"],
  "scripts": {
    "dev": "turbo dev",
    "build": "turbo build"
  }
}
```

### turbo.json
```json
{
  "$schema": "https://turbo.build/schema.json",
  "tasks": {
    "build": { "dependsOn": ["^build"], "outputs": [".next/**", "dist/**"] },
    "dev": { "cache": false, "persistent": true }
  }
}
```

### Install Stellar JS SDK
```bash
pnpm add @stellar/stellar-sdk
```

---

## Contract Architecture Summary

```
Registry
  ├── publish(operator_sig, auditor_sig, cert_data) → cert_id
  ├── verify(agent_address) → VerifyResult { valid, reserve, stake, limit }
  └── get_certificate(cert_id) → Certificate

SpendingLimit
  ├── initialize(agent, hard_limit, reserve_vault)
  ├── pay(amount, recipient) → tx_hash  [REVERTS if amount > hard_limit]
  ├── get_limit() → u128
  └── get_spent() → u128

ReserveVault
  ├── deposit(operator, amount)
  ├── get_balance() → u128
  ├── release_to_victim(victim, amount)  [only ChallengeManager]
  └── release_to_operator()  [only on cert expiry, no incidents]

AuditorStaking
  ├── stake(auditor, amount)
  ├── get_stake(auditor) → u128
  ├── slash(auditor, recipient, amount)  [only ChallengeManager]
  └── release(auditor)  [only on cert expiry, no incidents]

FeeEscrow
  ├── deposit(operator, auditor, amount)
  ├── release_to_auditor()  [after attestation]
  └── slash_to_challenger(challenger)  [if challenge wins]

ChallengeManager
  ├── challenge(cert_id, proof) + stake
  ├── evaluate(challenge_id) → Verdict
  ├── execute_slash(challenge_id)  [if fraud proven]
  └── return_stake(challenge_id)  [if challenge fails]
```

---

## Contract Data Types (Soroban / Rust)

```rust
pub struct Certificate {
    pub agent: Address,
    pub operator: Address,
    pub auditor: Address,
    pub hard_limit: i128,
    pub reserve_amount: i128,
    pub auditor_stake: i128,
    pub issued_at: u64,
    pub expires_at: u64,
    pub spending_limit_contract: Address,
    pub reserve_vault_contract: Address,
    pub auditor_staking_contract: Address,
}

pub struct VerifyResult {
    pub valid: bool,
    pub hard_limit: i128,
    pub reserve: i128,
    pub auditor_stake: i128,
    pub expires_at: u64,
}
```

---

## Demo Accounts (create fresh for hackathon)

| Role | Purpose |
|---|---|
| `operator` | Deploys contracts, deposits reserve, locks fee |
| `agent` | Makes payments via SpendingLimit |
| `auditor` | Stakes own USDC, signs attestation |
| `challenger` | Watches system, submits challenges |
| `counterparty` | Receives payments, verifies certificates |

Create all via Stellar CLI or Lab, fund via testnet faucet.

---

## Key Files to Create (in order)

```
contracts/reserve-vault/src/lib.rs
contracts/auditor-staking/src/lib.rs
contracts/fee-escrow/src/lib.rs
contracts/spending-limit/src/lib.rs
contracts/challenge-manager/src/lib.rs
contracts/registry/src/lib.rs

sdk/src/client.ts          (auto-generated bindings)
sdk/src/index.ts           (publishCertificate, verifyCertificate)
sdk/src/payments.ts        (executePayment, x402 integration)
sdk/src/demo.ts            (end-to-end demo script)

apps/web/app/page.tsx      (landing)
apps/web/app/dashboard/page.tsx
apps/web/app/demo/page.tsx
```

---

## Submission Checklist

- [ ] GitHub repo public with clear README
- [ ] All 6 contracts deployed to Stellar Testnet
- [ ] Contract addresses listed in README
- [ ] SDK published or importable from repo
- [ ] Web app deployed (Vercel or similar)
- [ ] Live demo works (agent pays → executes → blocks)
- [ ] Technical design doc written
- [ ] Pitch deck prepared (use hackathon template)
- [ ] Submitted on risein.com portal
- [ ] Ticked: Main Track
- [ ] Ticked: Hack Agentic

---

## Hack Agentic — Required Submission Answers

The judges explicitly require you to answer these three questions:

1. **Which actions does the agent perform autonomously?**
   > The agent autonomously makes payments up to the hard limit via SpendingLimit.pay(). All transactions below the limit execute without human approval.

2. **What safeguards are in place?**
   > Hard limit (code-enforced ceiling, cannot be exceeded), reserve vault (locked USDC covers losses), auditor staking (independent party staked capital on honest attestation), challenge mechanism (anyone can prove fraud and earn reward).

3. **Why is Stellar the right chain for this use case?**
   > x402 is Stellar-native — CCP becomes transparent middleware in the agentic payment stack. Native USDC means no bridging. 3-5s finality means certificate verification is near-instant. Sub-cent fees make micro-transactions viable.
