# Bound Protocol

> **A surety bond for AI agents, on-chain.**
> Built for the Build On Stellar Hackathon — IBW 2026 Istanbul · Main Track + Hack Agentic

Know your worst case _before_ you transact. AI agents now hold wallets and move money
autonomously, but reputation can't tell you your maximum downside — models change
silently and a fresh identity is free. Bound Protocol replaces the unanswerable
_"can I trust this agent?"_ with a number you can look up:

> **"Your worst-case loss is bounded at $X, it's pre-funded, and an independent auditor
> staked their own money that this is true."**

---

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
| [`docs/VERIFY.md`](./docs/VERIFY.md)       | How to verify the deployment yourself               |
| [`docs/RESOURCES.md`](./docs/RESOURCES.md) | Reference material and external links               |

---

## Architecture

Five Soroban (Rust) smart contracts on Stellar Testnet, a TypeScript SDK, an MCP
server, and a Next.js app. All value movement is in USDC (the testnet Circle SAC).

```
contracts/
├── registry/            # Store, publish, attest, verify Bound Certificates
├── reserve-vault/        # Locked USDC reserve — absorbs worst-case loss
├── auditor-staking/      # Auditor's own stake — slashable on fraud
├── fee-escrow/           # Conditional audit fee — released after attestation
└── challenge-manager/    # Dispute resolution — slash auditor + compensate victim

apps/dashboard/           # Next.js app + API routes + BoundClient SDK
bindings/                 # Generated TypeScript contract bindings
mcp/server.ts             # MCP server exposing the 5 agent tools
scripts/                  # setup-accounts, deploy-all, demo (8-step E2E), smoke suites
```

## Deployed contracts (Stellar Testnet)

| Contract          | Address                                                    |
| ----------------- | ---------------------------------------------------------- |
| Registry          | `CBM2UAVZFUI2QGZIS35VB6P3W5FYC3HW3KV3E2AF6KFQDUMFIZPPAJWV` |
| ReserveVault      | `CDN6S5DKUCC4O33L3RGTTO4LYNJVPLPIYYZTUANPJJZYAHZCR32O4WFB` |
| AuditorStaking    | `CCSJTEXOJZ322XI5ZJF6YZ2IRLCRKNXTGHGK3ZLATKL6Y7CGQODS4VZB` |
| FeeEscrow         | `CD4EZ5FCFC7D65OHBB5HHF6ASM4OCRGNAO6XQZQ5LKH4QB327SF5EYOF` |
| ChallengeManager  | `CANDUKOYQIMZDK4MUWHN6MKJI5ORY5R4BPBEQIOQQSINJYITMY47UNZH` |
| USDC (Circle SAC) | `CBIBCQ6EIQX3DU2SIJ3MFN7MQBNXBCETNLTFHNQOSTTLZDSGQLENPVWV` |

---

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
