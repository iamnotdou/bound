# Bound Protocol

> **A surety bond for AI agents, on-chain.**
> Originally built for the Build On Stellar Hackathon — IBW 2026 Istanbul.
> Now submitted to the Rise In × Stellar **Pro Hackathon 2026, Scale Track**.

Know your worst case _before_ you transact. AI agents now hold wallets and move money
autonomously, but reputation can't tell you your maximum downside — models change
silently and a fresh identity is free. Bound Protocol replaces the unanswerable
_"can I trust this agent?"_ with a number you can look up:

> **"Your worst-case loss is bounded at $X, it's pre-funded, and an independent auditor
> staked their own money that this is true."**

---

|                   |                                                                                                                                                                 |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Live demo**     | [www.boundprotocol.dev/app](https://www.boundprotocol.dev/app) — browse bonded agents, publish and fund a certificate, attest one                               |
| **Documentation** | [docs.boundprotocol.dev](https://docs.boundprotocol.dev)                                                                                                        |
| **SDK**           | [`@bound/sdk`](https://www.npmjs.com/package/@bound/sdk) — typed client for the whole lifecycle                                                                 |
| **MCP connector** | [`@bound/mcp`](https://www.npmjs.com/package/@bound/mcp) — 15 tools for any MCP-capable agent                                                                   |
| **Quickstart**    | [`docs/QUICKSTART.md`](./docs/QUICKSTART.md) — reproducible from a clean clone                                                                                  |
| **What is built** | [`docs/DELIVERABLES.md`](./docs/DELIVERABLES.md) — per-requirement audit, checked against source                                                                |
| **Trust model**   | [`docs/THREAT-MODEL.md`](./docs/THREAT-MODEL.md) and the [disclosed defects](https://docs.boundprotocol.dev/docs/trust-model#defects-in-the-deployed-contracts) |

### Repositories

|                                                         |                                                         |
| ------------------------------------------------------- | ------------------------------------------------------- |
| [`bound`](https://github.com/iamnotdou/bound)           | this repo — contracts, SDK, MCP connector, demo scripts |
| [`bound-web`](https://github.com/iamnotdou/bound-web)   | the marketing site and the app at `boundprotocol.dev`   |
| [`bound-docs`](https://github.com/iamnotdou/bound-docs) | the documentation site at `docs.boundprotocol.dev`      |

Testnet only. Amounts are testnet USDC and no mainnet deployment exists.

## How it works

1. **Lock the reserve.** The operator locks real USDC in a vault. It's the pre-funded
   payout, and it can't be withdrawn until the certificate expires.
2. **An auditor vouches — with their own money.** The instant they attest, their stake
   locks to the certificate. If the vouch is false, they lose it.
3. **Read before you transact.** Any counterparty calls `verify(agent)` and sees the
   bound, the locked reserve, the auditor's stake, and the status.
4. **Anyone can catch a lie — and gets paid.** If the reserve is short, the contract
   proves it itself and slashes the auditor in one transaction: 80% to the victim,
   20% to whoever caught it. No oracle. No judge. Just arithmetic.

The honest edge: a **short reserve** is provable on-chain with zero trusted parties.
Proving _who was harmed_ or _how much an agent spent_ can't be — those name a victim
or fall back to a named arbiter. The certificate tells you which is which.

Contributing, or pointing an agent at this repo? Start with
[`AGENTS.md`](./AGENTS.md) — commands, prohibitions, and the definition of done.

Full documentation lives in [`docs/`](./docs):

| Document                                   | What it covers                                      |
| ------------------------------------------ | --------------------------------------------------- |
| [`docs/WRITEUP.md`](./docs/WRITEUP.md)     | The academic framing, developer docs, and narrative |
| [`docs/PROJECT.md`](./docs/PROJECT.md)     | The build plan, trust model, and known limitations  |
| [`docs/RESOURCES.md`](./docs/RESOURCES.md) | Reference material and external links               |

---

## Architecture

Seven Soroban (Rust) contracts on Stellar Testnet, a published TypeScript SDK, a
published MCP connector, and a Next.js app in its own repo. All value moves in
USDC — the testnet Circle SAC — and no contract holds a key: every write is
authorised by the actor it belongs to.

```
contracts/
├── registry/            # Certificates: publish, attest, verify, invalidate, freeze
├── reserve-vault/       # The operator's locked USDC, walled off per certificate
├── auditor-staking/     # The auditor's stake, allocated per certificate, slashable
├── challenge-manager/   # Four proof types, the claim window, and settlement
├── payment-router/      # SEP-41 wrapped USDC that meters spend per certificate
├── premium-vault/       # Coverage premiums, accruing to the auditor as yield
├── fee-escrow/          # Deployed but unused — superseded by premium-vault
├── spend-probe/         # Never deployed: the executable proof that spend ≠ loss
└── integration-tests/   # Offline cross-contract harness

bindings/                # Generated TypeScript clients — never hand-edited
packages/sdk/            # @bound/sdk — the publishable typed client
packages/mcp/            # @bound/mcp — the agent tools, packaged as an MCP server
scripts/                 # setup, deploy, demo, evidence, anchor, and the smoke suites
deployments/             # testnet.json — the single source of truth for addresses
```

### System

```mermaid
flowchart TB
  operator([Operator]):::actor
  agent([AI agent]):::actor
  auditor([Auditor]):::actor
  challenger([Challenger]):::actor

  subgraph clients["Clients — nothing here holds a contract address of its own"]
    web["bound-web<br/>Next.js app"]
    mcpsrv["@bound/mcp<br/>MCP server + 15 tools"]
  end

  sdk["@bound/sdk<br/>typed client · committed deployment record"]
  kit["Stellar Wallets Kit<br/>Freighter · xBull · Lobstr · Albedo · Hana · Rabet"]
  anchor[["Anchor<br/>SEP-10 auth · SEP-24 deposit/withdraw"]]:::ext

  subgraph chain["Soroban — Stellar testnet"]
    registry["Registry"]
    vault["ReserveVault"]
    staking["AuditorStaking"]
    cm["ChallengeManager"]
    premium["PremiumVault"]
    router["PaymentRouter"]
    usdc[("USDC SAC")]
  end

  operator --> web
  auditor --> web
  challenger --> web
  agent --> mcpsrv

  web --> kit
  kit -- "signs the envelope<br/>the server assembled" --> web
  web --> sdk
  mcpsrv --> sdk
  sdk --> chain

  operator -. "fiat in" .-> anchor
  anchor -. "USDC out" .-> vault
  vault -. "payout" .-> anchor
  anchor -. "fiat out" .-> challenger

  registry -- "is_registered · allocate" --> staking
  cm -- "pay_from_reserve · get_balance" --> vault
  cm -- "slash_allocation · retire_allocation" --> staking
  cm -- "forfeit · terminate" --> premium
  cm -- "spent · post_expiry_spent" --> router
  cm -- "invalidate · set_claim_freeze" --> registry
  vault -- "get_cert_operator<br/>get_cert_settlement_deadline" --> registry
  router -- "cert terms, copied in at enroll" --> registry
  premium --> registry
  vault --> usdc
  staking --> usdc
  premium --> usdc
  router --> usdc

  classDef actor fill:#fff,stroke:#888,stroke-dasharray:3 3
  classDef ext fill:#fff7ed,stroke:#f97316
```

The dotted edges are the fiat rail: a reserve funded from a SEP-24 deposit, and a
proven claim paid out through a SEP-24 withdrawal. That is the boundary the
protocol is only useful across — a surety bond whose collateral cannot be funded
from, or redeemed to, money people actually spend is not a bond.

### Settlement

The part worth reading closely. One rule, applied identically to every proof
type, with the pots drawn in a fixed order:

```mermaid
sequenceDiagram
  autonumber
  actor C as Challenger
  participant CM as ChallengeManager
  participant R as Registry
  participant V as ReserveVault
  participant S as AuditorStaking
  participant P as PremiumVault

  C->>CM: challenge(cert, proof, victim, bond)
  CM->>R: read the certificate, verify the predicate from state
  alt wrong at filing
    CM-->>C: ChallengeFails — bond forfeited, decided in this same tx
  else proven
    CM->>R: set_claim_freeze — capital frozen, others may join
    Note over CM: 72-hour claim window
    CM->>V: get_balance
    CM->>S: get_allocation
    Note over CM: payable = min(harm, reserve + allocation)
    CM->>V: 1. pay_from_reserve → victim
    CM->>V: 2. pay_from_reserve → challenger fee, % of proven harm
    CM->>S: 3. slash_allocation → treasury, never victim or challenger
    CM->>P: 4. forfeit → victim, capped by uncovered harm
    CM->>S: 5. retire_allocation — unslashed remainder returns to free stake
    CM->>R: 6. invalidate — and the challenger's bond comes back
  end
```

Every line closes a specific attack; the reasons are attached to them in
[`contracts/challenge-manager/src/lib.rs`](./contracts/challenge-manager/src/lib.rs).
The one that matters most: **the auditor's slash goes to the treasury, never to
the victim or the challenger.** A colluding operator who files a challenge
against their own certificate must not be able to collect the auditor's stake.

### The claim the protocol refuses to make

`PaymentRouter::spent(cert_id)` is **gross routed flow, not loss**. The
arithmetic cannot be forged, so `spent > bound` is a sound predicate — and a
worthless settlement rule, because one dollar shuttled between two addresses the
same operator controls drives the counter past any bound for the price of gas.
[`contracts/spend-probe`](./contracts/spend-probe) is the executable proof and is
kept in the workspace for that reason. Anything that pays out is sized by harm
proven to a party outside the operator's control, and capped by the collateral
actually behind the certificate.

## Deployed contracts (Stellar Testnet)

Generated from [`deployments/testnet.json`](./deployments/testnet.json), which the
deploy script owns and `@bound/sdk/deployments` publishes. `test/readme-addresses.test.ts`
fails if this table and that record ever disagree, so an address here is an address
you can paste into an explorer.

| Contract         | Address                                                    | What it does                                                                   |
| ---------------- | ---------------------------------------------------------- | ------------------------------------------------------------------------------ |
| Registry         | `CCJTY2VYHQZ7OQE6NX7QFL7JL766YMTNKGHZXGAPTPKV5IUS2AK5UREF` | Certificates. `publish` → `attest` → `verify`.                                 |
| ReserveVault     | `CD6VK2YUUZO5L5R76DNT3NQNWR2UCALPQ3T6STWAQSEY7Q6I5P4LAFC3` | The operator's locked USDC, walled off per certificate.                        |
| AuditorStaking   | `CBPUCSASKMQKRWJ7WUTQ6KUPX66QUBV2E6V4CQ46BI4ZLJSW6AFPR6RB` | The auditor's own stake, allocated per certificate and slashable.              |
| ChallengeManager | `CAYEGPIHNDIEONWNKRF2UPTO32SXGFTLBQ2K4RPN2LCIGOOZYLYYYIHY` | Four proofs, a 72-hour claim window, pro-rata settlement.                      |
| PaymentRouter    | `CA5OPBLVGNPAFNFHY3ATNZXJ72L3K4EG4RIQYOWOKESAN7C75DDMMCMJ` | SEP-41 wrapped USDC that meters an enrolled agent's spend.                     |
| PremiumVault     | `CA5JT2IBPY7X4QZS65XY6YW2BXDUEFPZHXTNG2OCPCJTOM4DWEWEOHAF` | Coverage priced bound × duration, accruing to the auditor as yield.            |
| FeeEscrow        | `CAM3D5PTXQ4MEY45SGNUZZGNN2D6JXHSCFR3IM6S46KCPWERWJGDFXNI` | Deployed but unused — superseded by PremiumVault, kept rather than redeployed. |
| USDC             | `CDIQ4564SFRCLBP2UL4Z5IBGEBDF2J5ISQVCTHN3F5SNVDXGSB5YEAMM` | The testnet Circle Stellar Asset Contract. Every amount moves in this token.   |

Deployed 2026-08-21 from commit [`82e34af8b9e8`](https://github.com/iamnotdou/bound/commit/82e34af8b9e8b4456623b46bda4b32016f8abc4f).
Read-only simulations use the funded source account `GDOUNKJLAMAPLK7IZ2MGBE3S4RHF4SSQYPDRCGK2VNSP6IHFR5OQ7HGF`.

---

## Stellar Skills used

The [official skill files](https://skills.stellar.org/) that bear on the code in
this repo, cited by path as the submission requires. Each line names the part of
the codebase it applies to, so a reader can check the claim rather than take it.

| Skill file                  | Where it applies                                                                                                                                                                                                                                                                                                          |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `skills/anchors/SKILL.md`   | [`scripts/anchor-deposit.ts`](./scripts/anchor-deposit.ts), [`scripts/anchor-status.ts`](./scripts/anchor-status.ts), [`scripts/anchor-trustlines.ts`](./scripts/anchor-trustlines.ts) — SEP-10 challenge/sign/JWT, the SEP-24 interactive deposit, and the classic trustline the anchor's asset needs before it can land |
| `skills/standards/SKILL.md` | The SEP choices themselves: SEP-41 for [`contracts/payment-router`](./contracts/payment-router), SEP-10/24 for the fiat rail, and why `transfer` may make no sub-invocation                                                                                                                                               |

Two more are relevant to work in flight rather than to code already here, and are
cited only if they end up in the diff: `skills/integration-finder/SKILL.md` for
choosing the second ecosystem integration, and `skills/soroswap/SKILL.md` if the
TRY↔USDC swap at the anchor boundary lands.

## Running it

```bash
pnpm install
pnpm verify            # everything CI runs — offline, no secrets, ~6s
```

`pnpm verify` is the definition of done: typecheck, lint, formatting, unit
tests, and the contract fmt/clippy/test suites. It needs no network and no
credentials, so a fresh clone can run it immediately.

The individual pieces, if you want to run one on its own:

```bash
# TypeScript
pnpm test              # unit tests, offline
pnpm typecheck
pnpm lint              # eslint .
pnpm format:check      # prettier

# Contracts
pnpm test:contracts    # cargo test — 21 tests across 5 contracts
pnpm lint:contracts    # cargo clippy -D warnings
pnpm build:contracts   # 5 wasm artifacts
```

Two commands spend testnet funds and mutate on-chain state, so they are kept
out of `verify` and are never automated. Both need `.env.testnet`:

```bash
pnpm test:e2e          # 5 live smoke suites against testnet
pnpm demo              # 8-step: stake → reserve → fee → attest → publish → verify → pay → SLASH
```

Contract addresses and network endpoints are committed in
[`deployments/testnet.json`](./deployments/testnet.json), so `pnpm verify`,
`pnpm build` and `pnpm dev` need no configuration at all.

For the live commands, copy `.env.example` to `.env.testnet` and fill in the
five account secret keys. **Never commit `.env*` files** — they hold secret keys.

---

## License

[MIT](./LICENSE)
