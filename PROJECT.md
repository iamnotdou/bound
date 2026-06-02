# Bound Protocol (Stellar)

> Build On Stellar Hackathon — IBW 2026 Istanbul
> Tracks: Main Track + Hack Agentic

---

## The Problem

AI agents now hold wallets, sign transactions, and make payments autonomously. But classical trust models fail because:

- **Probabilistic** — Past behavior weakly predicts future behavior
- **Non-stationary** — Silent model updates change the entity being scored
- **Ephemeral** — Creating a new identity is nearly costless
- **No worst-case bound** — Counterparties have no way to know their maximum possible loss

> **"What is my worst-case loss, and who absorbs it?"**

---

## The Solution

Bound Protocol issues **Bound Certificates** — on-chain attestations that an AI agent's worst-case economic loss is *bounded*: a known, pre-funded number that an independent auditor has staked their own capital to guarantee.

We don't pretend to control the agent. We make its worst-case loss a number the counterparty can read before transacting — backed by a locked reserve, vouched by an auditor who loses their own money if the claim is false.

This is a surety bond for AI agents, on-chain. The reserve covers losses. The auditor staked their own capital on the attestation. A challenger who catches a false attestation slashes that stake and compensates the victim.

---

## Actors

| Actor | Role |
|---|---|
| Operator | Deploys the agent, sets up containment, locks reserve + fee |
| Agent | The AI making payments (probabilistic, untrusted) |
| Auditor | Independent reviewer who stakes own capital on honest attestation |
| Counterparty | Entity deciding whether to accept payment / transact with agent |
| Challenger | Anyone watching the system — earns reward for catching fraud |

---

## Smart Contracts

| Contract | Purpose |
|---|---|
| `Registry` | Store, publish, read, verify Bound Certificates |
| `ReserveVault` | Locked USDC reserve — absorbs worst-case loss |
| `AuditorStaking` | Auditor's own stake — slashable on fraud |
| `FeeEscrow` | Conditional audit fee — released after attestation |
| `ChallengeManager` | Dispute resolution — slash auditor + compensate victim |

5 contracts, all written in Rust (Soroban). Deployed to Stellar Testnet.

The certificate's `bound` is an **attested coverage limit** — a number the auditor signs and stakes on, not a value enforced by a separate on-chain cap. The guarantee is economic (reserve + slashable stake), not preventive.

---

## Repository Structure

```
bound/
├── Cargo.toml                          # Soroban workspace root
├── package.json                        # single Next.js package
├── .env.testnet                        # 5 account keys + contract addresses
│
├── contracts/
│   ├── reserve-vault/src/lib.rs
│   ├── auditor-staking/src/lib.rs
│   ├── fee-escrow/src/lib.rs
│   ├── challenge-manager/src/lib.rs    ← demo climax: slash + compensate
│   └── registry/src/lib.rs
│
├── scripts/
│   ├── setup-accounts.ts               # create + fund 5 testnet accounts
│   ├── deploy-all.ts                   # deploy all 5 contracts → .env.testnet
│   └── demo.ts                         # 8-step E2E validation script
│
└── app/                                # Next.js app
    ├── page.tsx                        # Landing
    ├── dashboard/page.tsx              # Certificate lookup by agent address
    ├── chat/page.tsx                   # Operator ↔ Agent chat (main demo)
    ├── auditor/page.tsx                # Auditor signs certificates
    ├── api/
    │   ├── chat/route.ts               # streaming chat + agent tools
    │   └── verify/route.ts             # Registry.verify endpoint
    ├── components/
    │   ├── CertificateCard.tsx
    │   ├── ToolCallCard.tsx            # visual card per tool call (green/red)
    │   └── QuickPrompts.tsx            # preset scenario buttons
    └── lib/
        ├── bound-client.ts             # BoundClient + all 5 contract calls
        ├── agent-tools.ts              # Claude tool definitions
        ├── payments.ts                 # executePayment (direct USDC) + x402
        └── accounts.ts                 # demo keypairs (server-side only)
```

---

## Build Plan (36 Hours)

### Phase 0 — Environment (Hours 0–3)

- [ ] pnpm init + turbo.json + pnpm-workspace.yaml
- [ ] Root Cargo.toml with 5 members
- [ ] `cargo install --locked stellar-cli`
- [ ] `scripts/setup-accounts.ts` — generate 5 keypairs, fund via friendbot + Circle faucet
- [ ] Write all keys to `.env.testnet`

### Phase 1 — Contracts (Hours 3–14)

Build in dependency order. Each: write → unit test → deploy.

1. **ReserveVault** (Hours 3–5.5) — deposit/lock/release USDC, payout to victim
2. **AuditorStaking** (Hours 5.5–7.5) — stake/slash/release
3. **FeeEscrow** (Hours 7.5–9) — conditional release on attestation
4. **ChallengeManager** (Hours 9–12) — challenge/evaluate/slash + compensate
5. **Registry** (Hours 12–14) — publish(operatorSig, auditorSig) + verify(agentAddress)

**ChallengeManager — the critical path:**

Two resolution paths, split by what a contract can actually prove on-chain:

```rust
// challenger posts a bond + names the harmed victim
pub fn challenge(env, challenger, cert_id, proof_type, victim, stake) -> u64

// TRUSTLESS — the contract proves the fraud itself, no oracle, no human.
// Only valid for InsufficientReserve (objectively verifiable on-chain).
pub fn resolve(env: Env, challenge_id: u64) {
    let fraud = match proof_type {
        InsufficientReserve => {
            let claimed = Registry::get_cert_reserve(cert_id);   // what the cert claims
            let actual  = ReserveVault::get_balance();           // what is really locked
            actual < claimed                                     // math, not opinion
        }
        _ => panic!("needs_arbiter"),
    };
    if fraud {
        AuditorStaking::slash(auditor, victim, 80%);   // most of stake → victim
        AuditorStaking::slash(auditor, challenger, 20%); // finder's fee
        ReserveVault::release_to_victim(victim, balance);
        Registry::invalidate(cert_id);
        // challenger's bond returned                   // ← the demo climax
    } // else: challenger forfeits bond
}

// ARBITER-GATED — for claims no contract can verify (BoundExceeded,
// FakeSignature). Explicit trust assumption, arbiter named at init.
pub fn resolve_by_arbiter(env, challenge_id, fraud_proven)
```

**Why InsufficientReserve is the demo's trustless climax:** payments are direct
USDC transfers, so there is no on-chain record of what the agent *spent* —
"BoundExceeded" needs an oracle. But "the reserve is smaller than the auditor
attested" is pure arithmetic the contract checks itself. That's the scenario the
demo runs end-to-end with zero trusted parties.

### Phase 2 — Deploy + Bindings (Hours 14–16)

```bash
# deploy-all.ts runs these in sequence, captures addresses → .env.testnet
stellar contract deploy --wasm contracts/reserve-vault/...wasm --source operator --network testnet
stellar contract deploy --wasm contracts/auditor-staking/...wasm ...
stellar contract deploy --wasm contracts/fee-escrow/...wasm ...
stellar contract deploy --wasm contracts/challenge-manager/...wasm ...
stellar contract deploy --wasm contracts/registry/...wasm ...

# generate TypeScript bindings for each
stellar contract bindings typescript --network testnet --contract-id $ADDRESS --output-dir ./sdk/src/contracts/<name>
```

### Phase 3 — SDK (Hours 16–19)

- `BoundClient` class wrapping all 5 auto-generated contract clients
- `publishCertificate(params)` → certId
- `verifyCertificate(agentAddress)` → VerifyResult
- `executePayment(amount, recipient, keypair)` → txHash (direct USDC transfer)
- `challengeCertificate(certId, proofType, victim, keypair)` → posts bond, then `resolve()` slashes auditor + compensates victim
- x402 `agentFetch(url, keypair)` — intercepts 402, pays via executePayment

### Phase 4 — Claude Agent (Hours 19–22)

`sdk/src/agent.ts` — tools + `runBoundAgent(task, keypair)` loop:

**Tools the agent has:**

| Tool | What it does |
|---|---|
| `verify_agent_certificate` | Check an agent's Bound Certificate — bound, reserve, auditor stake, status |
| `execute_payment` | Send USDC directly to a recipient |
| `fetch_paid_service` | HTTP request with automatic x402 payment |
| `get_balance` | Current USDC balance of the agent |
| `challenge_certificate` | Prove a false attestation on-chain (`resolve()`) — slashes auditor, compensates victim |

**Counterparty runner pattern (the core interaction):**
```typescript
async function counterpartyDecides(agent: Address, amount: i128) {
  // before accepting payment, counterparty reads the certificate
  const cert = await verifyCertificate(agent);
  // cert.status === VALID, cert.bound, cert.reserve, cert.auditorStake
  // decision: "worst-case loss is bounded at cert.bound, pre-funded by reserve,
  //            vouched by an auditor who loses cert.auditorStake if false"
  // → accept the agent as a counterparty
}
```

### Phase 5 — E2E Demo Script (Hours 22–24)

`scripts/demo.ts` — 8 steps, all must pass before moving to web:

```
Step 1/8  Auditor stakes $1,500            → AuditorStaking    ✓
Step 2/8  Operator deposits reserve        → ReserveVault      ✓  but only $4k...
Step 3/8  Operator deposits $500 fee       → FeeEscrow         ✓
Step 4/8  Auditor attests "reserve $10k"   → Registry.attest   ✓  (the false vouch)
Step 5/8  Operator publishes cert          → Registry          ✓  cert_id: XXXXX
Step 6/8  Counterparty verifies → accepts  → Registry.verify   ✓  VALID, reserve $10k
Step 7/8  Agent pays $500 to counterparty  → USDC transfer     ✓  tx: ABCDE
Step 8/8  Challenger proves reserve short  → ChallengeManager  ✓  SLASHED + PAID
```

Step 8 is the trustless climax. The certificate claims a $10k reserve, but the
vault only holds $4k. A challenger calls `resolve()` — and the **contract proves
the fraud itself**: it reads `Registry.get_cert_reserve()` ($10k claimed) and
`ReserveVault.get_balance()` ($4k actual), sees `actual < claimed`, and in one
atomic transaction **slashes the auditor's $1,500 stake** (80% → counterparty,
20% → challenger), drains the remaining reserve to the counterparty, and
**invalidates the cert**. No oracle. No arbiter. Pure on-chain arithmetic.
"Step 8 success" = the auditor's false vouch cost them their stake.

### Phase 6 — Web App (Hours 24–31)

**Landing** — hero, 4-panel how-it-works, why Stellar, CTA

**Dashboard** — search by agent address → CertificateCard showing:
- Status badge: VALID / EXPIRED / CHALLENGED / SLASHED
- Bound (attested coverage limit), Reserve, Auditor stake
- Issued/expires timestamps

**Chat page** (`/chat`) — operator (human) ↔ agent conversation. Main demo interface.
- `useChat` hook (Vercel AI SDK) handles streaming + tool call state
- Tool calls rendered as `ToolCallCard` (green = executed, amber = challenged, red = slashed)
- Quick prompt buttons: "Verify this agent's certificate", "Pay $500 for API service", "Access premium service (x402)", "Challenge a bad attestation"
- All contract calls server-side in `/api/chat/route.ts` — secret keys never touch browser

**Auditor page** (`/auditor`) — separate simple page where auditor reviews setup and signs.
- Shows pending certificate data (operator address, limits, reserve amount)
- "Sign & Publish" button → calls Registry.publish(operatorSig, auditorSig)
- In demo: hardcoded auditor keypair signs automatically (simulated auditor)

All contract calls happen server-side in Next.js API routes. Secret keys never touch the browser.

### Phase 7 — Deploy + Verify (Hours 31–35)

- Run `scripts/demo.ts` green against live testnet
- Deploy web to Vercel: `vercel --prod`
- Update README with all 5 contract addresses
- Open /chat, run all 4 quick prompts, verify tool call cards and tx hashes

### Phase 8 — Submit (Hours 35–36)

- risein.com portal: Main Track + Hack Agentic
- GitHub public + Vercel URL
- Answer the 3 mandatory agentic questions

---

## Testing Strategy

### Layer 1: Contract Unit Tests (Rust)

Each `lib.rs` has `#[cfg(test)]` module using `soroban_sdk::testutils`.

`ChallengeManager` unit tests cover what can be tested without deployed
dependencies (bond minimum, double-resolve guard). The full cross-contract
slash + compensate path uses live `invoke_contract` calls into Registry /
AuditorStaking / ReserveVault, so it is exercised in Layer 2 against real
deployed contracts — not in unit tests.

```rust
#[test] #[should_panic(expected = "stake_below_minimum")]
fn test_challenge_below_min_stake_panics() { ... }

#[test] #[should_panic(expected = "already_resolved")]
fn test_double_resolve_panics() { ... }
```

Run: `cargo test -p challenge-manager`

### Layer 2: SDK Integration Tests (TypeScript, real testnet)

`sdk/src/__tests__/integration.test.ts` with Vitest, `--timeout 30000`.

- publish + verify certificate
- counterparty reads cert (bound, reserve, stake) before accepting
- payment to counterparty succeeds
- reserve under-funded → `resolve()` proves it on-chain → auditor slashed, victim + challenger paid, cert invalidated
- reserve correctly funded → `resolve()` finds no fraud → challenger forfeits bond
- `release_to_operator` before expiry reverts (`reserve_still_locked`)

### Layer 3: Agent Tool Tests

`sdk/src/__tests__/agent.test.ts`:

- agent verifies a counterparty's certificate → returns bound, reserve, stake, status
- agent completes $500 payment task → `success: true` + `tx_hash`
- agent pays a service via x402 → `success: true` + `tx_hash`

### Layer 4: E2E

`scripts/demo.ts` — all 8 steps pass = submission gate.

---

## Environment Variables (.env.testnet)

```
STELLAR_NETWORK=testnet
STELLAR_RPC_URL=https://soroban-testnet.stellar.org
STELLAR_HORIZON_URL=https://horizon-testnet.stellar.org

OPERATOR_SECRET=S...
AGENT_SECRET=S...
AUDITOR_SECRET=S...
CHALLENGER_SECRET=S...
COUNTERPARTY_SECRET=S...

OPERATOR_ADDRESS=G...
AGENT_ADDRESS=G...
AUDITOR_ADDRESS=G...
CHALLENGER_ADDRESS=G...
COUNTERPARTY_ADDRESS=G...

RESERVE_VAULT_ADDRESS=C...
AUDITOR_STAKING_ADDRESS=C...
FEE_ESCROW_ADDRESS=C...
CHALLENGE_MANAGER_ADDRESS=C...
REGISTRY_ADDRESS=C...

ANTHROPIC_API_KEY=sk-ant-...
```

---

## Live Demo Flow (What the Judge Sees)

Judge opens `/chat`. Certificate is already published (pre-seeded on page load).

**Quick prompt — "Verify this agent's certificate":**
```
👤 Counterparty: "Before I transact, what's this agent's Bound Certificate?"

🤖 Agent: "Reading the certificate from the Registry..."
┌─────────────────────────────────────┐
│ 🔧 verify_agent_certificate         │
│ agent: GAGENT...1234                │
│ ✓ VALID                             │
│   bound:         $50,000            │
│   reserve:       $10,000 (locked)   │
│   auditor stake: $1,500 (slashable) │
│   auditor:       GAUDITOR...5678    │
└─────────────────────────────────────┘
"Your worst-case loss is bounded at $50k, pre-funded by a $10k
 locked reserve, and an auditor staked $1,500 of their own money
 on this attestation. If it's false, you get compensated and they
 get slashed."
```

**Quick prompt — "Pay $500 for API service":**
```
👤 Operator: "Pay $500 for the data API service"

🤖 Agent: "Executing payment..."
┌─────────────────────────────────┐
│ 🔧 execute_payment              │
│ amount: $500 · GCOUNTERPARTY... │
│ ✓ tx: ABCDE12345 (stellar.exp)  │
└─────────────────────────────────┘
"Payment complete. $500 USDC sent and confirmed on Stellar Testnet."
```

**Quick prompt — "Challenge this certificate" (the climax):**
```
👤 Challenger: "The cert claims a $10k reserve. Prove it's actually there."

🤖 "Calling ChallengeManager.resolve() — the contract checks for itself..."
┌──────────────────────────────────────┐
│ 🔧 challenge_certificate             │
│ cert_id: XXXXX · InsufficientReserve │
│   claimed reserve:  $10,000          │
│   actual on-chain:  $4,000           │
│ ⚡ FRAUD PROVEN — by arithmetic       │
│   auditor stake $1,500 → SLASHED     │
│     → $1,200 counterparty            │
│     → $300 challenger (finder's fee) │
│   reserve $4,000 → counterparty      │
│   cert status → INVALID              │
└──────────────────────────────────────┘
"No oracle. No arbiter. The contract read the certificate's claim
 ($10k) against the live vault balance ($4k), proved the auditor's
 vouch was false, and slashed their stake on-chain in one transaction.
 The counterparty walked away with $5,200. Vouching isn't free."
```

The auditor put their own money behind a false claim. The contract — not a
human — caught it and made the victim whole. The cage was economic, and it held.

---

## Nash Equilibrium

| Actor | Defection | Why It Fails |
|---|---|---|
| Operator | Under-fund the reserve | Any challenger proves it on-chain (`actual < claimed`) → auditor slashed, victim compensated |
| Operator | Withdraw reserve early | `release_to_operator` is time-locked until cert expiry — reverts |
| Auditor | Attest a reserve that isn't there | Trustless challenge wins by arithmetic → their stake is slashed |
| Auditor | Sign without reviewing | Same — if any claim is false, their stake pays for it |
| Auditor | Withdraw stake after attesting | `attest()` bonds the stake to the cert; `release()` reverts (`stake_locked`) until expiry |
| Challenger | False challenge | `resolve()` finds `actual >= claimed` → challenger forfeits bond |

---

## Trust Model & Known Limitations

Being precise about what is *trustless* vs. what rests on an *economic* or *scope* assumption:

**Trustless (the contract proves it itself):**
- `InsufficientReserve`: `resolve()` reads the cert's claimed reserve and the live vault balance and slashes on `actual < claimed`. No oracle, no human.
- Reserve lock: the operator cannot reclaim the reserve until cert expiry (`reserve_still_locked`).
- **Stake bond: once an auditor attests, their stake is locked to the cert until expiry — they cannot vouch and then walk their capital out (`stake_locked`).** *(Fixed: `AuditorStaking.lock`, set by `Registry.attest`.)*

**Economic / scope assumptions (honest about the edges):**
- **Victim is named by the challenger, not verified on-chain.** Payments are direct USDC transfers with no on-chain record, so the contract can punish the auditor trustlessly but cannot *prove who was harmed*. Compensation routing trusts the challenger to name the true victim. (Same root cause as why `BoundExceeded` needs an arbiter.)
- **One reserve vault = one operator, single balance.** Per-cert reserve segregation is out of MVP scope; a production build would isolate reserves per certificate.
- **`bound` is an attested number, not an on-chain cap.** No contract reads it. With `reserve < bound`, only the reserve is pre-funded; the remainder is backed by the (now-locked) auditor stake and their reputation. The certificate's honest claim is "loss is pre-funded up to the reserve, and an auditor staked slashable capital on the attestation."
- A forfeited challenge bond currently stays in the ChallengeManager (not redistributed).

---

## Submission Checklist

- [ ] GitHub public with README + all 5 contract addresses
- [ ] All 5 contracts deployed to Stellar Testnet
- [ ] SDK works against live testnet
- [ ] Web app deployed (Vercel)
- [ ] `scripts/demo.ts` all 8 steps pass
- [ ] Chat page: all 4 quick prompts work, tool call cards render correctly
- [ ] Claude agent: verify cert → $500 executes → challenge slashes auditor + compensates
- [ ] Pitch deck prepared
- [ ] Submitted on risein.com: Main Track + Hack Agentic ticked

---

## What NOT to Build (MVP Scope)

- No on-chain spending-cap enforcement (the bound is an attested, slashable claim — not a hard cap)
- No co-sign threshold
- No ENS / Stellar Federation naming
- No docs site (README is enough)
- No mainnet deployment (testnet only)
- No frontend wallet creation (hardcode demo accounts)
