# Bound Protocol — Writeup

Five layers of the same idea, zoomed from formal to copy:

1. [Academic: Problem & Solution](#1--academic-problem--solution)
2. [High-Level Explanation](#2--high-level-explanation)
3. [Developer Docs](#3--developer-docs)
4. [Narrative](#4--narrative-the-story-of-one-certificate)
5. [Landing Page Narrative](#5--landing-page-narrative-copy)

---

## 1 · Academic: Problem & Solution

### 1.1 The problem: trust has no worst-case bound

Counterparty decisions about an economic actor have always rested on a prediction:
*will this party honor its obligation?* For human institutions we answer it with
reputation, credit scores, collateral, and legal recourse — instruments that work
because the underlying entity is **stable, identifiable, and accountable over time.**

Autonomous AI agents that hold wallets and sign transactions break all three
assumptions simultaneously:

- **Probabilistic.** An agent's behavior is sampled from a model, not guaranteed by
  a contract. Past good behavior is weak evidence about the next action.
- **Non-stationary.** A silent weight update or prompt change can replace the entity
  you scored with a different one wearing the same address. The thing you trusted no
  longer exists, but its identity persists.
- **Ephemeral / Sybil-cheap.** Spinning up a fresh keypair costs nothing. Reputation
  that can be abandoned and re-minted for free is not a cost, so it cannot deter.

The result is that a counterparty transacting with an agent cannot answer the only
question that actually governs the decision:

> **"What is my worst-case loss, and who absorbs it?"**

Every existing approach answers a *different*, weaker question. Reputation systems
estimate `P(bad behavior)` — a probability, not a bound. Spending caps constrain the
agent's *own* wallet but say nothing about a counterparty's exposure and can be
bypassed by deploying a fresh agent. Insurance exists off-chain but reintroduces a
trusted intermediary and a claims process. None of them produce a **pre-funded,
verifiable, worst-case number** that a counterparty can read *before* transacting.

### 1.2 Reframing: from prediction to bounded liability

Bound Protocol does not try to make the agent trustworthy. It abandons prediction
entirely and replaces it with **bounded, pre-funded, slashable liability** — the same
structure as a **surety bond**, expressed on-chain.

A surety bond has three parties: a principal who must perform, an obligee who is
protected, and a surety who guarantees performance and pays out if the principal
fails. Bound maps this directly onto the agent economy:

| Surety bond | Bound Protocol |
|---|---|
| Principal | Operator + Agent (the party that must not cause loss) |
| Obligee | Counterparty (the party protected against loss) |
| Surety | Auditor (stakes own capital to vouch) + Reserve (pre-funded payout) |
| Bond amount | `bound` — attested worst-case coverage |
| Premium | Audit fee (escrowed, released on attestation) |
| Claim | Challenge (proves the vouch was false, triggers payout) |

The key move is **substituting verification for prediction.** We do not estimate the
probability the agent misbehaves. We make the *consequence* of the worst case a
concrete, locked number, and we make the entity that vouched for that number lose
their own capital if the number is a lie.

### 1.3 The economic object: a Bound Certificate

A **Bound Certificate** is an on-chain attestation with three load-bearing scalars:

- `bound` — the attested worst-case loss ceiling (e.g. $50,000).
- `reserve` — capital actually locked and pre-funded to absorb loss (e.g. $10,000).
- `auditor_stake` — the auditor's *own* capital, slashable if the attestation is
  false (e.g. $1,500).

A counterparty reads `verify(agent)` and obtains a closed-form answer to the worst-case
question: *loss is pre-funded up to `reserve`, and beyond that an independent auditor
has staked `auditor_stake` of their own money on the claim being honest.* The decision
is no longer "do I trust this agent?" but "is `bound` large enough, and is the backing
credible?" — a question with numbers in it.

### 1.4 What is provable on-chain vs. what rests on an assumption

The protocol is rigorous about the boundary between **trustless** (a contract proves
it from state, no human) and **economic/scope** assumptions. This honesty is the
academic contribution: it isolates exactly the one fraud type that is *objectively
verifiable* and makes only that one fully trustless.

**Trustless (the contract proves it itself):**

- **`InsufficientReserve`.** Whether the locked reserve is smaller than the
  certificate claims is *pure arithmetic over on-chain state*: compare
  `Registry.get_cert_reserve(cert)` (claimed) against `ReserveVault.get_balance()`
  (actual). `actual < claimed` is fraud, decided by math, with no oracle and no
  parameters supplied by the challenger.
- **Reserve lock.** The operator cannot reclaim the reserve before certificate
  expiry — enforced by a timestamp check, not a promise.
- **Stake bond.** The instant an auditor attests, their stake is locked to the
  certificate until expiry. They cannot vouch and then withdraw the capital out from
  under the counterparty.

**Economic / scope assumptions (stated, not hidden):**

- **`BoundExceeded` needs an arbiter.** Payments are direct USDC transfers, so there
  is *no on-chain record of what the agent spent.* The chain cannot prove total
  spend exceeded `bound`; that claim routes to a named arbiter — an explicit trust
  assumption, not a disguised one.
- **Victim is named by the challenger.** Because there is no on-chain record of who
  was harmed, the contract can punish the auditor trustlessly but must trust the
  challenger to name the true victim for compensation routing. (Same root cause as
  `BoundExceeded`.)
- **One reserve vault = one operator.** Per-certificate reserve segregation is out of
  MVP scope.

### 1.5 The mechanism design: a Nash equilibrium toward honesty

The system is engineered so that for every actor, honesty dominates defection:

| Actor | Defection | Why it fails |
|---|---|---|
| Operator | Under-fund the reserve | Any challenger proves `actual < claimed` on-chain → auditor slashed, victim paid |
| Operator | Withdraw reserve early | `release_to_operator` is time-locked until expiry → reverts |
| Auditor | Attest a reserve that isn't there | Trustless challenge wins by arithmetic → their stake is slashed |
| Auditor | Sign without reviewing | Same — any false claim costs them their stake |
| Auditor | Withdraw stake after attesting | `attest()` bonds the stake to the cert → `release()` reverts (`stake_locked`) |
| Challenger | False challenge | `resolve()` finds `actual ≥ claimed` → challenger forfeits their bond |

The challenger role is the enforcement engine: a finder's fee (20% of the slashed
stake) turns fraud-detection into a *paid, permissionless* activity. Anyone watching
the chain has a profit motive to catch a short reserve, which is what makes the
auditor's honesty self-enforcing rather than merely hoped-for.

### 1.6 Contribution, in one sentence

Bound Protocol shows that for AI agents you do not need to *predict* trustworthiness
if you can *bound liability* — and that at least one fraud class (a short reserve) can
be proven and punished entirely on-chain, converting "trust me" into "the math
slashed them."

---

## 2 · High-Level Explanation

**The one-liner:** Bound Protocol is a surety bond for AI agents. Before you transact
with an agent, you can read a certificate that says "your worst-case loss is $X, and
it's pre-funded — and an independent auditor staked their own money that this is true."

**Why you'd want it.** AI agents now move money on their own. The problem isn't that
they're evil; it's that you have no way to know your maximum downside before you deal
with one. Reputation doesn't help — an agent can be silently updated or replaced for
free. Bound replaces the unanswerable "can I trust this agent?" with a number you can
look up: "what's my worst case, and who pays it?"

**How it works, in four beats:**

1. **Lock the reserve.** The agent's operator locks real money (USDC) in a vault. This
   is the pre-funded payout if things go wrong. It can't be pulled out until the
   certificate expires.

2. **Get an auditor to vouch — with their own money.** An independent auditor reviews
   the setup and attests to a certificate. The moment they sign, their personal stake
   is locked to that certificate. They now have skin in the game: if their vouch turns
   out to be false, they lose it.

3. **Counterparties read before they transact.** Anyone about to deal with the agent
   looks up `verify(agent)` and sees the bound, the locked reserve, the auditor's
   stake, and whether the cert is still valid. They make an informed decision.

4. **Anyone can catch a lie — and get paid for it.** If the reserve is actually smaller
   than the certificate claims, *any* observer can challenge. The contract checks for
   itself: it reads what the cert claims, reads what's really in the vault, and if the
   vault is short it **slashes the auditor's stake in one transaction** — 80% to the
   victim, 20% to the challenger as a finder's fee — drains the reserve to the victim,
   and marks the cert dead. No oracle. No human judge. Just arithmetic.

**The thing to remember:** Bound doesn't stop the agent from doing anything. It makes
lying about the safety net *expensive for the person who vouched for it.* The cage is
economic, and it's enforced by math, not trust.

**An honest edge:** the chain can prove a *short reserve* with zero trusted parties.
It cannot prove *who got harmed* (payments leave no on-chain "victim" record) — so the
challenger names the victim, and a couple of harder fraud types fall back to a named
arbiter. We're upfront about exactly where trustlessness ends.

---

## 3 · Developer Docs

### 3.1 Architecture

Five Soroban (Rust) smart contracts on Stellar Testnet, plus a TypeScript SDK and a
Next.js app. All value movement is in USDC (the testnet Circle SAC).

```
                       ┌─────────────────────────────────────────┐
                       │              Registry                    │
                       │  publish() → attest() → verify()         │
   operator ──publish──▶  stores Certificate{bound, reserve,...}  │
   auditor  ──attest───▶  on attest: locks auditor stake to cert  │
                       └───┬───────────────┬─────────────────┬────┘
                           │ get_cert_*     │ lock()          │ invalidate()
                           ▼                ▼                 │
        ┌──────────────────────┐  ┌─────────────────────┐    │
        │   ReserveVault        │  │  AuditorStaking      │    │
        │  deposit / locked     │  │  stake / lock /slash │    │
        │  release_to_victim    │  │  release (gated)     │    │
        └──────────┬────────────┘  └──────────┬──────────┘    │
                   │ release_to_victim         │ slash         │
                   │                           │               │
                   └──────────┐     ┌──────────┘    ┌──────────┘
                              ▼     ▼               ▼
                       ┌─────────────────────────────────┐
                       │        ChallengeManager          │
                       │  challenge() → resolve()         │
                       │  proves InsufficientReserve,     │
                       │  slashes + compensates in one tx │
                       └─────────────────────────────────┘
              FeeEscrow (audit fee, released on attestation) sits alongside.
```

### 3.2 Contracts

All amounts are `i128` in stroops-style fixed point: **USDC has 7 decimals**, so
`$1,500 = 1_500_0000000`.

#### `Registry` — the certificate store

| Function | Auth | Purpose |
|---|---|---|
| `initialize(challenge_manager, auditor_staking)` | once | wire dependencies |
| `publish(operator, agent, bound, reserve_amount, expires_at, reserve_vault, auditor_staking) -> cert_id` | operator | create a `Pending` cert |
| `attest(auditor, cert_id)` | auditor | verify registered → `Verified`; **calls `AuditorStaking.lock(auditor, expires_at)`** |
| `verify(agent) -> VerifyResult` | view | `{valid, status, bound, reserve, auditor_stake, auditor, expires_at}` |
| `get_cert_reserve(cert_id) -> i128` | view | claimed reserve — read by ChallengeManager |
| `get_cert_auditor(cert_id) -> Address` | view | who vouched — read by ChallengeManager |
| `invalidate(cert_id)` | ChallengeManager only | mark `Invalid` |

`CertStatus`: `Pending` (operator published, awaiting auditor) → `Verified` (auditor
attested) → `Invalid` (challenged & slashed). `verify().valid` is true only when
`status == Verified && now <= expires_at`.

#### `ReserveVault` — the pre-funded payout

| Function | Auth | Notes |
|---|---|---|
| `initialize(operator, challenge_manager, token, unlock_at)` | once | `unlock_at` = cert expiry |
| `deposit(amount)` | operator | locks reserve in the vault |
| `get_balance() -> i128` | view | **actual** locked balance — the on-chain truth |
| `release_to_victim(victim, amount)` | ChallengeManager only | compensation path |
| `release_to_operator()` | operator | **reverts `reserve_still_locked` if `now < unlock_at`** |

MVP scope: one vault = one operator, single balance (no per-cert segregation).

#### `AuditorStaking` — slashable skin in the game

| Function | Auth | Notes |
|---|---|---|
| `initialize(challenge_manager, registry, token, min_stake)` | once | |
| `stake(auditor, amount)` | auditor | stake ≥ `min_stake` ⇒ counts as registered |
| `is_registered(auditor) -> bool` | view | `stake >= min_registration_stake` |
| `get_stake(auditor) -> i128` | view | current live stake |
| `lock(auditor, until)` | **Registry only** | bond stake to a cert; extends, never shortens |
| `locked_until(auditor) -> u64` | view | timestamp the bond lifts |
| `slash(auditor, recipient, amount)` | ChallengeManager only | move stake → recipient |
| `release(auditor)` | auditor | **reverts `stake_locked` if `now < locked_until`** |

#### `ChallengeManager` — the trustless climax

| Function | Auth | Notes |
|---|---|---|
| `initialize(registry, auditor_staking, reserve_vault, fee_escrow, token, arbiter, min_stake)` | once | |
| `challenge(challenger, cert_id, proof_type, victim, stake) -> challenge_id` | challenger | posts a bond ≥ `min_stake` |
| `resolve(challenge_id)` | **permissionless** | trustless path — only `InsufficientReserve` |
| `resolve_by_arbiter(challenge_id, fraud_proven)` | arbiter only | subjective types (`BoundExceeded`, `FakeSignature`) |

`resolve()` for `InsufficientReserve` (`verify_insufficient_reserve`):

```rust
let claimed: i128 = invoke Registry.get_cert_reserve(cert_id);   // what the cert claims
let actual:  i128 = invoke ReserveVault.get_balance();           // what's really locked
let fraud = actual < claimed;                                    // math, not opinion
```

On `fraud == true` (`settle_fraud`), in **one transaction**:

1. read the cert's auditor and their **live** stake;
2. `slash(auditor, victim, 80%)` and `slash(auditor, challenger, 20%)` (`reward = stake / 5`);
3. drain remaining reserve via `release_to_victim(victim, balance)`;
4. `Registry.invalidate(cert_id)`;
5. return the challenger's bond.

On `fraud == false`, the challenger forfeits their bond (stays in the contract).

#### `FeeEscrow` — conditional audit fee

Operator deposits the audit fee naming the auditor; released on attestation. Not on
the slash path.

### 3.3 TypeScript SDK / app layer

```ts
import { BoundClient } from "@/app/lib/bound-client";

const bound = new BoundClient(/* config from .env.testnet */);

// counterparty reads before transacting
const cert = await bound.verifyCertificate(agentAddress);
// → { valid, status, bound, reserve, auditorStake, auditor, expiresAt }

// agent pays a counterparty (direct USDC transfer)
const { txHash } = await bound.executePayment(amount, recipient, agentKeypair);

// x402: HTTP request with automatic 402 → pay → retry
const res = await bound.agentFetch(url, agentKeypair);

// challenger proves a short reserve on-chain
const challengeId = await bound.challengeCertificate(certId, "InsufficientReserve", victim, challengerKeypair);
await bound.resolve(challengeId); // slashes auditor + compensates victim
```

**Agent / MCP tools** (5, exposed to Claude and any MCP client):
`verify_agent_certificate`, `get_balance`, `execute_payment`, `fetch_paid_service`
(x402), `challenge_certificate`.

**HTTP routes:** `/api/verify` (cert lookup), `/api/paid-service` (x402 402→pay→200),
`/api/auditor` (pending params + sign), `/api/chat` (streaming agent — needs
`ANTHROPIC_API_KEY`).

### 3.4 Running it

```bash
# Contracts
cargo test                                          # 21 unit tests
cargo build --release --target wasm32-unknown-unknown   # 5 wasm artifacts

# TypeScript + live testnet smoke
pnpm typecheck
pnpm verify-all        # typecheck + tools/sdk/mcp/routes smoke suites

# E2E demo (spends testnet funds, mutates state — run intentionally)
pnpm demo              # 8-step: stake → reserve → fee → attest → publish → verify → pay → SLASH
```

Config + the 5 account keys + 5 contract addresses live in `.env.testnet` (git-ignored).
Never print secret keys; never commit `.env*`.

### 3.5 Integration recipe (a counterparty)

```ts
async function shouldITransact(agent: Address, exposure: bigint): Promise<boolean> {
  const c = await bound.verifyCertificate(agent);
  if (!c.valid) return false;               // expired / challenged / unknown
  if (BigInt(c.bound) < exposure) return false;  // bound too small for my exposure
  // reserve is pre-funded; auditor staked c.auditorStake on this being true.
  return true;
}
```

---

## 4 · Narrative (the story of one certificate)

An operator has built a trading agent. It's good — but "good" is a probability, and the
counterparties it wants to do business with don't deal in probabilities. They deal in
worst cases. So the operator does something an honest party would do and a fraudster
would fake: they get the agent **bonded.**

They lock $10,000 of USDC into a reserve vault. The money is real and it isn't going
anywhere — the vault won't release it back to them until the certificate expires. Then
they bring in an auditor.

The auditor's job is to vouch. But a vouch is worthless if it's free, so Bound makes it
expensive: the moment the auditor attests to the certificate, $1,500 of *their own*
money locks to it. They can't take it back until the cert expires. They've just put
skin in the game, on-chain, where everyone can see it. The certificate goes live: it
claims a **$50,000 bound, $10,000 reserve, $1,500 auditor stake.**

Now a counterparty shows up. Before sending anything, they read the certificate —
`verify(agent)` — and get the only answer that matters: *worst case is bounded, it's
pre-funded up to the reserve, and someone independent staked their own capital that
this is true.* That's a decision they can make. They transact.

Here's where it gets interesting. Suppose the certificate is a lie — it claims a
$10,000 reserve, but the vault only really holds $4,000. In the old world, you'd find
out too late, file a claim, and argue with someone. In Bound, a challenger — anyone
watching the chain, motivated by a finder's fee — calls `resolve()`.

The contract doesn't ask anyone's opinion. It reads what the certificate claims
($10,000). It reads what the vault actually holds ($4,000). $4,000 < $10,000. That's
fraud, proven by arithmetic, and the contract acts on it in a single transaction: it
**slashes the auditor's $1,500 stake** — $1,200 to the harmed counterparty, $300 to the
challenger who caught it — drains the remaining reserve to the victim, and stamps the
certificate **INVALID.**

No oracle was consulted. No arbiter ruled. No human decided anything. The auditor put
their own money behind a false claim, and the contract — not a person — caught it and
made the victim whole. The counterparty walked away compensated. The auditor learned
that vouching isn't free.

That's the whole thesis in one transaction: **you can't make an AI agent trustworthy,
but you can make lying about its safety net cost the liar their own capital — and you
can let math, not trust, do the enforcing.** The cage was economic, and it held.

---

## 5 · Landing Page Narrative (copy)

### Hero

> # Know your worst case before you transact.
>
> AI agents move money on their own. Bound Protocol tells you the one thing reputation
> can't: **your maximum loss — pre-funded, and backed by someone who loses their own
> money if they lied.**
>
> `[ Read a live certificate ]`   `[ See the slash happen ]`

### The problem (one line)

> You can't predict an AI agent. You *can* bound what it costs you.

Reputation is probabilistic, models change silently, and a fresh identity is free. None
of that answers the only question that matters when money is on the line: **what's my
worst case, and who absorbs it?**

### How it works — 4 panels

**1. Lock the reserve.**
The operator locks real USDC in a vault. It's the pre-funded payout, and it can't be
withdrawn until the certificate expires.

**2. An auditor vouches — with their own money.**
An independent auditor attests to the certificate. The instant they sign, their stake
locks to it. If the vouch is false, they lose it.

**3. Read before you transact.**
Any counterparty looks up the certificate: bound, locked reserve, auditor's stake,
status. A worst-case number you can actually act on.

**4. Anyone can catch a lie — and gets paid.**
If the reserve is short, the contract proves it itself and slashes the auditor in one
transaction: 80% to the victim, 20% to whoever caught it. No oracle. No judge. Just math.

### The climax (show, don't tell)

> **The certificate claimed a $10,000 reserve. The vault held $4,000.**
>
> A challenger called `resolve()`. The contract read the claim, read the real balance,
> and — `$4,000 < $10,000` — proved the fraud by arithmetic. In one transaction it
> slashed the auditor's $1,500 stake, paid the victim, and killed the certificate.
>
> **No oracle. No arbiter. Vouching isn't free.**

### Why Stellar

> Real USDC, sub-cent fees, and Soroban smart contracts that can read each other's
> state in a single atomic transaction — which is exactly what a trustless slash needs.

### Honest by design

> We're precise about where trustlessness ends. A *short reserve* is proven on-chain
> with zero trusted parties. Proving *who was harmed*, or *how much an agent spent*,
> can't be — so those name a victim or fall back to an arbiter. We tell you which is
> which, on the certificate.

### Closing CTA

> # The cage is economic. And it holds.
>
> `[ Verify an agent ]`   `[ Read the docs ]`   `[ View contracts on Stellar ]`
