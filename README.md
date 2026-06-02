# Bound Protocol

> **A surety bond for AI agents, on-chain.**
> Built for the Build On Stellar Hackathon — IBW 2026 Istanbul · Main Track + Hack Agentic

Know your worst case *before* you transact. AI agents now hold wallets and move money
autonomously, but reputation can't tell you your maximum downside — models change
silently and a fresh identity is free. Bound Protocol replaces the unanswerable
*"can I trust this agent?"* with a number you can look up:

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
Proving *who was harmed* or *how much an agent spent* can't be — those name a victim
or fall back to a named arbiter. The certificate tells you which is which.

See [`WRITEUP.md`](./WRITEUP.md) for the full academic framing, developer docs, and
narrative; [`PROJECT.md`](./PROJECT.md) for the build plan; [`VERIFY.md`](./VERIFY.md)
for verification steps.

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

app/                      # Next.js app + API routes + BoundClient SDK
bindings/                 # Generated TypeScript contract bindings
mcp/server.ts             # MCP server exposing the 5 agent tools
scripts/                  # setup-accounts, deploy-all, demo (8-step E2E), smoke suites
```

## Deployed contracts (Stellar Testnet)

| Contract | Address |
|---|---|
| Registry | `CAXMUURGPXX3UGGQIFIOLHX65WBIDLC4ZHBM2R3LNTBRSVRO65WWUOUY` |
| ReserveVault | `CD6DMEEWNRR576GCD5TL5K3QU3SUPC5WBOSISUYAIWXIMKC4VKWHH2LR` |
| AuditorStaking | `CAJAXWV3N2TAYRQMM6UZZMDODZUWCMZDJXW7HA6GXKZLFDEKUMSLNZVA` |
| FeeEscrow | `CCDZSM4YFXDSDYKY6MPTGG5PZSIQQAZQ3NCDI4F2SRQIXL4IZLJLG3CZ` |
| ChallengeManager | `CCK7XU2NUUSGRV3Z5Y4W6BBS53SFCHM3AFHIKPDQVR5CCJIRQCWRMB3S` |
| USDC (Circle SAC) | `CBIBCQ6EIQX3DU2SIJ3MFN7MQBNXBCETNLTFHNQOSTTLZDSGQLENPVWV` |

---

## Running it

```bash
# Contracts
cargo test                                              # unit tests
cargo build --release --target wasm32-unknown-unknown   # 5 wasm artifacts

# TypeScript + live testnet smoke
pnpm install
pnpm typecheck
pnpm verify-all        # typecheck + tools/sdk/mcp/routes smoke suites

# E2E demo (spends testnet funds, mutates state — run intentionally)
pnpm demo              # 8-step: stake → reserve → fee → attest → publish → verify → pay → SLASH
```

Copy `.env.example` to `.env.testnet` and fill in your account keys + contract
addresses. **Never commit `.env*` files** — they hold secret keys.

---

## License

[MIT](./LICENSE)
