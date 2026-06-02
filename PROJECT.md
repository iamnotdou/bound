# CCP — Containment Certificate Protocol (Stellar)

> Build On Stellar Hackathon — IBW 2026 Istanbul
> Tracks: Main Track + Hack Agentic

---

## The Problem

AI agents now hold wallets, sign transactions, and make payments autonomously. But classical trust models fail because:

- **Probabilistic** — Past behavior weakly predicts future behavior (LLM outputs are stochastic)
- **Non-stationary** — Silent model updates change the entity being scored
- **Ephemeral** — Creating a new identity is nearly costless, so punishment is meaningless
- **No worst-case bound** — Counterparties have no way to know their maximum possible loss

The question a counterparty needs answered is not "will this agent behave well?" It is:

> **"What is my worst-case loss, and who absorbs it?"**

---

## The Solution

CCP is a protocol for issuing, publishing, and verifying **containment certificates** — on-chain attestations that an AI agent's economic impact is structurally bounded by agent-independent constraints, backed by exogenous reserves.

A certificate answers one question:

> Does the surrounding system make this agent economically safe enough to transact with?

The cage holds. Even if the agent goes rogue, the smart contract hard limit is absolute. The reserve covers losses. The auditor staked their own money on the claim.

---

## Core Concepts

### Agent-Independent vs Agent-Influenceable

| Type | Examples | Can Agent Degrade It? |
|---|---|---|
| Agent-Independent | Smart contract limits, multi-sig thresholds | No — code is code |
| Agent-Influenceable | Human oversight, reputation scores, rate limits | Yes — agent can social-engineer |

**Design rule:** Even if ALL agent-influenceable layers collapse, agent-independent layers must bound the loss to a level the reserve can absorb.

Only agent-independent constraints count toward the containment bound.

### The Hard Limit

The agent does not hold funds directly. All payments flow through the `SpendingLimit` contract:

```
$0 → $X        agent signature alone → executes
$X → hard max  NOT SUPPORTED in MVP (co-sign removed for simplicity)
above hard max  always REVERT — no signature can override this
```

This means: the agent cannot spend more than `hard_max` under any circumstances. The contract code enforces it. The agent cannot change the contract.

---

## Full System Flow

### Actors

| Actor | Role |
|---|---|
| Operator | Deploys the agent, sets up containment, locks reserve + fee |
| Agent | The AI making payments (probabilistic, untrusted) |
| Auditor | Independent reviewer who stakes own capital on honest attestation |
| Counterparty | Entity deciding whether to accept payment/transact with agent |
| Challenger | Anyone watching the system — earns reward for catching fraud |

---

### Phase 1 — Setup (done once per agent)

```
OPERATOR
  │
  ├─► Deploy SpendingLimit contract
  │     - agent address
  │     - hard limit: e.g. $50,000 (absolute ceiling, code-enforced)
  │
  ├─► Lock $10,000 USDC in ReserveVault
  │     - operator cannot withdraw
  │     - released only on certificate expiry or by ChallengeManager
  │
  └─► Lock $500 USDC in FeeEscrow (audit fee)
        - released to auditor only after honest attestation


AUDITOR
  │
  ├─► Stake $1,500 own USDC in AuditorStaking  ← skin in the game
  │
  ├─► Review the setup:
  │     - Are limits real and correctly set?
  │     - Is reserve deposited and locked?
  │     - Is the operator address correct?
  │
  ├─► If satisfied: sign attestation
  │
  └─► FeeEscrow releases $500 to auditor


OPERATOR
  └─► Registry.publish(operatorSig, auditorSig)
        → Certificate is now on-chain, publicly readable
```

---

### Phase 2 — Runtime (every transaction)

```
Agent decides to pay $500
  │
  └─► SpendingLimit.pay(500 USDC, recipient)
        │
        ├─ amount ≤ hard_limit?
        │    YES → execute ✓
        │
        └─ amount > hard_limit?
             → always REVERT ✗
                (impossible by code, no workaround)
```

The agent's funds are held by the contract, not the agent wallet. The agent can only call the contract. It cannot bypass it.

---

### Phase 3 — Verification (counterparty checks before transacting)

```
Counterparty: "This agent wants to send me $3,000. Should I accept?"
  │
  └─► Registry.verify(agentAddress)
        │
        ├─ Certificate exists?                  ✓
        ├─ Operator signature valid?            ✓
        ├─ Auditor signature valid?             ✓
        ├─ Auditor stake still locked?          ✓  ($1,500)
        ├─ Reserve sufficient?                  ✓  ($10,000)
        ├─ $3k < hard limit?                    ✓  ($50,000 limit)
        ├─ Certificate not expired?             ✓
        │
        └─► PASS → accept the transaction

Counterparty reasoning:
  "Even if the agent goes rogue, it can't exceed $50k.
   $10k reserve exists to cover my loss.
   Auditor staked $1,500 — they lose it if they lied.
   My $3k transaction is safe."
```

---

### Phase 4 — Challenge (if something goes wrong)

```
Challenger notices: fraudulent certificate / insufficient reserve / limit exceeded
  │
  └─► ChallengeManager.challenge(certId, proof)
        + small stake (spam prevention)

ChallengeManager evaluates:

  CHALLENGE WINS:
    ├─► AuditorStaking → slash auditor's $1,500
    ├─► slashed amount → reward to challenger
    ├─► ReserveVault → compensate harmed counterparty
    └─► certificate marked invalid

  CHALLENGE LOSES:
    └─► challenger's stake is slashed
```

---

## Nash Equilibrium

Each actor's dominant strategy when everyone else plays honestly:

| Actor | Defection Scenario | Why It Fails |
|---|---|---|
| Operator | Set fake limits ($500k instead of $50k) | Auditor catches it — no attestation, no certificate |
| Operator | Don't deposit reserve | Auditor won't sign. Counterparty verify fails. |
| Auditor | Sign without reviewing | If system is exploited, stake slashed + reputation destroyed |
| Auditor | Collude with operator | Challenger watches — challenge wins, both slashed |
| Agent | Exceed hard limit | Contract code prevents it — physically impossible |
| Counterparty | Trust without verifying | Their own risk — verified users are protected |
| Challenger | Challenge valid certificate | Loses stake |
| Challenger | Ignore real fraud | Misses reward |

**No actor benefits from unilateral deviation. Nash equilibrium holds.**

---

## Stellar-Specific Design

### Why Stellar

| Feature | CCP Benefit |
|---|---|
| 3-5s finality | Certificate verification is near-instant |
| Sub-cent fees | Micro-transactions viable, agent payments cheap |
| Native USDC (Circle) | Real stablecoin, no mock needed |
| Native multi-sig | Threshold accounts replace hardware co-signers |
| Soroban | Expressive enough for all 6 contracts |
| x402 protocol | Agents can pay for HTTP services through CCP limits |

### x402 Integration (Killer Feature)

x402 is Stellar's native agentic payment protocol. When an agent makes an HTTP request to a service that costs money:

```
Normal flow (no CCP):
Agent → HTTP request → 402 Payment Required → agent pays directly → service

CCP + x402 flow:
Agent → HTTP request → 402 Payment Required
  → agent calls SpendingLimit.pay() via x402
  → if amount ≤ hard_limit → executes → service responds
  → if amount > hard_limit → REVERT → agent cannot proceed
```

This makes CCP directly pluggable into real agentic commerce. The agent doesn't need to know about CCP — the SpendingLimit contract is transparent middleware.

---

## Smart Contracts

| Contract | Purpose |
|---|---|
| `Registry` | Store, publish, read, verify certificates |
| `SpendingLimit` | Hard limit enforcement — agent pays through this |
| `ReserveVault` | Locked USDC reserve + audit fee bucket |
| `AuditorStaking` | Auditor's own stake — slashable on fraud |
| `FeeEscrow` | Conditional audit fee — released after attestation |
| `ChallengeManager` | Dispute resolution — slash + compensate |

All contracts written in Rust (Soroban). Deployed to Stellar Testnet.

---

## Repository Structure

```
ccp-stellar/
├── contracts/
│   ├── registry/
│   ├── spending-limit/
│   ├── reserve-vault/
│   ├── auditor-staking/
│   ├── fee-escrow/
│   └── challenge-manager/
├── sdk/
│   └── src/
│       ├── index.ts           publishCertificate, verifyCertificate
│       ├── payments.ts        executePayment (x402 compatible)
│       └── client.ts          Soroban contract clients (auto-generated)
├── apps/
│   └── web/                   Next.js — landing + dashboard + live demo
├── Cargo.toml
├── package.json
└── turbo.json
```

---

## End-to-End Ship Plan

### Step 0 — Environment Setup (Day 1, Hour 0-2)

- [ ] Init monorepo: `pnpm init` + `turbo.json`
- [ ] Install Stellar CLI: `cargo install --locked stellar-cli`
- [ ] Create 4 Stellar testnet accounts: operator, agent, auditor, challenger
- [ ] Fund all accounts via testnet faucet
- [ ] Get testnet USDC from Circle faucet
- [ ] Scaffold Soroban workspace: `stellar contract init`

### Step 1 — Core Contracts (Day 1, Hour 2-10)

Write and test contracts in this order (each depends on the previous):

1. **ReserveVault** — simplest, just deposit/lock/release USDC
2. **AuditorStaking** — stake, slash, release
3. **FeeEscrow** — deposit, release on condition, slash on condition
4. **SpendingLimit** — pay(), enforce hard limit, track spend
5. **ChallengeManager** — challenge(), evaluate(), slash(), compensate()
6. **Registry** — publish(operatorSig, auditorSig), verify(agentAddress)

For each contract:
- Write `src/lib.rs`
- Write unit tests in `#[cfg(test)]`
- Deploy to testnet: `stellar contract deploy`
- Note deployed contract address

### Step 2 — Integration Test (Day 1, Hour 10-13)

Write one end-to-end script (TypeScript) that runs the full demo flow:

1. Auditor stakes → `AuditorStaking.stake(1500)`
2. Operator deposits reserve → `ReserveVault.deposit(10000)`
3. Operator deposits fee → `FeeEscrow.deposit(500)`
4. Auditor attests → signs certificate data
5. Operator publishes → `Registry.publish(operatorSig, auditorSig)`
6. Counterparty verifies → `Registry.verify(agentAddress)` → PASS
7. Agent pays $500 → `SpendingLimit.pay(500)` → EXECUTES
8. Agent tries $80,000 → `SpendingLimit.pay(80000)` → REVERT

All 8 steps must pass before moving to web app.

### Step 3 — x402 Integration (Day 1, Hour 13-16)

- Implement `sdk/src/payments.ts` wrapping x402 + SpendingLimit
- Write a mock HTTP service that returns `402 Payment Required`
- Agent calls mock service → x402 triggers SpendingLimit → payment executes or blocks
- This becomes the centerpiece of the demo

### Step 4 — Web App (Day 1 Hour 16 — Day 2 Hour 8)

Single Next.js app with 3 pages:

**Page 1: Landing**
- What is CCP? (one paragraph)
- The problem: AI agents need trust infrastructure
- The solution: structural containment, not behavioral reputation
- CTA: "See Live Demo"

**Page 2: Dashboard**
- Connect wallet (Passkey-Kit or Freighter)
- Search agent address → show certificate details
- Certificate card shows:
  - Hard limit
  - Reserve amount
  - Auditor stake
  - Certificate status (VALID / EXPIRED / CHALLENGED)
  - Risk contribution breakdown (Reserve: 50%, Limit: 30%, Auditor: 20%)

**Page 3: Live Demo**
- Step-by-step interactive demo
- Button: "Agent pays $500" → show tx hash → PASS
- Button: "Agent pays $80,000" → show REVERT → BLOCKED
- Button: "Verify Certificate" → show all green checks
- Button: "Challenger attacks" → show slash

### Step 5 — Deploy & Verify (Day 2, Hour 8-10)

- Deploy all 6 contracts to Stellar Testnet
- Note all contract addresses
- Update SDK with deployed addresses
- Run full demo script against live testnet
- Verify everything passes

### Step 6 — Polish & Submission (Day 2, Hour 10-13)

- Write README: what it is, how to run, deployed addresses
- Write technical design doc (judges read this)
- Record 2-minute demo video
- Prepare 5-minute pitch (template from hackathon)
- Submit on risein.com portal
  - Tick: Main Track
  - Tick: Hack Agentic

---

## Demo Script (5-minute pitch)

```
0:00 - 0:45  The problem
  "AI agents now hold wallets. How do you trust an AI?"
  "You can't. But you can trust the cage."

0:45 - 1:30  The certificate
  "CCP issues a containment certificate — an on-chain proof
   that the agent cannot exceed its limits, backed by real reserves."

1:30 - 3:00  Live demo
  - Agent pays $500 → executes
  - Agent pays $80k → BLOCKED on chain
  - Counterparty verifies certificate → all green

3:00 - 3:45  Why Stellar + x402
  "x402 makes this transparent middleware —
   agents pay for HTTP services, CCP silently enforces limits."

3:45 - 4:30  Nash equilibrium
  "Auditor stakes own money → honest review is dominant strategy
   Challenger earns from fraud → system self-polices"

4:30 - 5:00  Ask
  "We're the safety layer for agentic commerce on Stellar."
```

---

## Judging Criteria Alignment

| Criteria | Our Answer |
|---|---|
| Meaningful idea | AI agent trust is the defining infrastructure problem of 2026 |
| Real-world impact | Every autonomous agent doing payments needs this |
| Technical implementation | 6 Soroban contracts, full test suite, deployed testnet |
| User experience | One-click verify, dashboard, live demo |
| Ecosystem fit | x402 native, Soroban, Circle USDC, Passkey-Kit |
| Agentic track | Autonomous spending, clear safeguards, x402 integration |

---

## What NOT to Build (MVP Scope)

- No co-sign threshold (hard limit is sufficient for demo)
- No ENS / Stellar Federation naming (use raw addresses)
- No docs site (README is enough)
- No mainnet deployment (testnet only)
- No frontend wallet creation flow (hardcode demo accounts)
