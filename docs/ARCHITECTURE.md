# Architecture

> Seven Soroban contracts, one asset, and the rail that connects them to fiat.
> Every contract ID below is live on Stellar testnet and linked to the explorer.

---

## System

```mermaid
graph TB
  subgraph fiat["Fiat boundary"]
    BANK["Bank / card<br/>off-ramp rails"]
    ANCHOR["SEP-24 anchor<br/>testanchor.stellar.org"]
  end

  subgraph actors["Actors"]
    OP["Operator<br/>deploys the agent"]
    AGT["Agent<br/>untrusted, holds a wallet"]
    AUD["Auditor<br/>stakes own capital"]
    CHL["Challenger<br/>earns a finder's fee"]
    CPY["Counterparty<br/>decides whether to transact"]
  end

  subgraph clients["Clients"]
    WEB["Dashboard — Next.js<br/>Stellar Wallets Kit"]
    SDK["@bound/sdk<br/>typed lifecycle client"]
    MCP["@bound/mcp<br/>15 tools, any MCP agent"]
  end

  subgraph chain["Stellar — Soroban contracts"]
    REG["Registry<br/>publish · attest · verify · invalidate"]
    RV["ReserveVault<br/>locked worst-case cover"]
    AS["AuditorStaking<br/>slashable stake"]
    FE["FeeEscrow<br/>conditional audit fee"]
    CM["ChallengeManager<br/>proof + settlement waterfall"]
    PR["PaymentRouter<br/>meters agent spend"]
    PV["PremiumVault<br/>prices and accrues coverage"]
  end

  USDC["USDC SAC<br/>anchor-issued, 7 decimals"]

  BANK <--> ANCHOR
  ANCHOR -- "SEP-10 auth · SEP-24 deposit" --> USDC
  USDC -- "SEP-24 withdraw · proven claim pays out" --> ANCHOR

  OP --> WEB
  AUD --> WEB
  CHL --> WEB
  CPY --> WEB
  AGT --> MCP
  WEB --> SDK
  MCP --> SDK
  SDK --> REG

  REG --> CM
  REG --> AS
  RV --> REG
  RV --> CM
  AS --> CM
  AS --> REG
  FE --> CM
  CM --> REG
  CM --> AS
  CM --> RV
  CM --> FE
  CM -. "set_router (one-shot)" .-> PR
  CM -. "set_premium_vault (one-shot)" .-> PV
  PR --> REG
  PV --> REG
  PV --> CM

  RV --- USDC
  AS --- USDC
  FE --- USDC
  CM --- USDC
  PR --- USDC
  PV --- USDC

  AGT -- "every payment routed" --> PR

  classDef contract fill:#1e3a5f,stroke:#4a90d9,color:#fff
  classDef anchor fill:#3d2f5f,stroke:#9b7fd4,color:#fff
  classDef asset fill:#1f4f3f,stroke:#4db892,color:#fff
  class REG,RV,AS,FE,CM,PR,PV contract
  class ANCHOR,BANK anchor
  class USDC asset
```

**The one edge that is new for this hackathon** is the dotted fiat boundary at the top.
Everything below it was deployed and verified before; the anchor turns Bound's reserve
from a mock token into money a person can put in and take out.

---

## Lifecycle — fiat in, proof, fiat out

```mermaid
sequenceDiagram
  autonumber
  actor OP as Operator
  participant AN as SEP-24 Anchor
  participant RV as ReserveVault
  participant REG as Registry
  actor AUD as Auditor
  participant AS as AuditorStaking
  participant PR as PaymentRouter
  actor CHL as Challenger
  participant CM as ChallengeManager

  Note over OP,AN: Fiat in
  OP->>AN: SEP-10 challenge, signed by the wallet kit
  AN-->>OP: JWT
  OP->>AN: SEP-24 interactive deposit
  AN-->>RV: anchor USDC credited on completion

  Note over OP,AS: Bond
  OP->>REG: publish(cert) — bound, expiry, reserve
  OP->>RV: deposit — locked until expiry + challenge window
  AUD->>AS: stake — own capital, slashable
  AUD->>REG: attest(cert) — verifies the reserve is actually funded

  Note over PR: Operation
  PR->>PR: every agent payment metered against the bound

  Note over CHL,CM: Proof
  CHL->>CM: challenge(cert, reason) + bond
  CM->>REG: read the claim
  CM->>RV: read the live balance
  CM->>PR: read spend vs. bound
  CM-->>CM: InsufficientReserve · BoundExceeded · ExpiredCertificate<br/>all proven by arithmetic, no oracle

  Note over CM,AN: Settlement, then fiat out
  CM->>AS: slash the auditor's stake
  CM->>RV: pay the victim from the reserve
  CM->>REG: invalidate the certificate
  RV-->>AN: SEP-24 withdraw — proven claim pays fiat
```

Three of the four challenge reasons are **trustless** — the contract reads the
certificate's claim against live on-chain state and settles without an arbiter.
Only `FakeSignature` needs one.

---

## Deployments

One deployment exists today. `deployments/testnet.json` holds a self-issued
test USDC (`CDIQ4564…`) and is what the live app, the published SDK and the
InstAward record all cite. Its contract ids are tabulated in the
[README](../README.md#deployed-contracts-stellar-testnet), generated from that
file rather than copied out of it.

A second deployment holding the anchor's USDC (`CBIELTK6…`) is the intended
next step, and is deliberately a _second_ deployment rather than a redeploy: the
docs site, the npm package and an approved grant SOW all cite the current
contract ids, and the anchor work must not move them.

`@bound/sdk` is ready for it — `NETWORK_NAMES` in `packages/sdk/src/deployments.ts`
is the single source of truth for the set, so adding one is the import, the
tuple and the record, in that one file. The deployment record itself is not
written yet, and until it is, a completed anchor deposit funds a wallet and
cannot fund a reserve. `/app/fiat` in the bound-web repo states that rather than
implying otherwise, and `pnpm check:anchor` there prints the two issuers side by
side.

### Scaling to the anchor's limits

The reference anchor caps every transfer at 10 units. The v2 scenario assumes
`AUDITOR_MIN_STAKE = 500` and `CHALLENGE_MIN_BOND = 100`, which nobody can fund
out of a max-10 deposit. Rather than hardcode a second set of constants,
`BOUND_AMOUNT_SCALE` divides every USDC figure in the deploy and demo scripts by
one factor, so every ratio the assertions depend on survives — `DE_MINIMIS_FLOOR_BPS`
is in basis points, so it scales with the bound rather than against it. Unset,
every existing script behaves exactly as before.

---

## Stellar skill files used

Cited once, in the [README](../README.md#stellar-skills-used), against the code
each one applies to. A second table here would be a second thing to keep in step
— and the first version of it had already gone stale, pointing at
`apps/dashboard/app/lib/wallet/kit.ts` after the frontend moved to its own
repository.
