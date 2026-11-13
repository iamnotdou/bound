# Bound on NEAR

NEAR HACKATHON integration of the Bound protocol. Everything in `near/` and
`packages/near/` was written in the hackathon window on branch
`near-hackathon`; the Soroban contracts in `contracts/` are prior work.

Bound lets a counterparty check an AI agent's worst-case loss before dealing
with it: a pre-funded USDC reserve, an auditor's slashable stake locked behind
that one certificate, and a spend meter the agent's NEAR key cannot route
around. If the agent causes harm, a claim window opens and one atomic
settlement pays the victim from the reserve and slashes the auditor for the
rest.

## Layout

| Path                       | What                                                                                                                                                               |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `contracts/bound`          | the protocol, one near-sdk 5.30.1 contract (`src/lib.rs`), settlement math (`src/settle.rs`), unit tests (`src/tests.rs`), near-sandbox tests (`tests/sandbox.rs`) |
| `contracts/mock-usdc`      | NEP-141/145/148 token, 6 decimals, open `mint` (testnet play money)                                                                                                |
| `deployments/testnet.json` | contract ids, deploy txs, wasm sha256 + on-chain code hash, demo runs with every tx hash                                                                           |
| `evidence/`                | committed demo logs: `near-demo-<run>.txt` (human) + `.json` (machine), gate ledger and invoices                                                                   |
| `scripts/verify.sh`        | build + test + checks, optional demo replay                                                                                                                        |
| `DESIGN.md`                | Soroban to NEAR deviations, trust model, limits, test map, failure modes                                                                                           |
| `../packages/near`         | `@bound/near`: near-api-js 7.5.3 client, deploy, demo, gate, status/verify/evidence                                                                                |

## Try it

Prerequisites: Rust via rustup (the pinned 1.95.0 toolchain installs itself),
`cargo-near` 0.23, node 22, pnpm 10. Run from the repo root unless noted.

1. Full lifecycle offline, no network, no keys:
   `cd near && cargo test`
   - see `test result: ok. 96 passed` (unit), `2 passed` (near-sandbox:
     `full_lifecycle_settles_victim`, `withdraw_failure_recredits_and_guards_hold`),
     `4 passed` (mock token). The first run downloads the sandbox binary.
2. Everything at once: `near/scripts/verify.sh`
   - builds both wasm, fmt/clippy, all tests, TS typecheck + vitest, checks the
     built wasm sha256 equals the deployed one, then the two zero-secret reads
     below. Ends with `== all checks passed`, exit 0.
3. Zero-secret testnet reads (no `~/.near-credentials` needed):
   - `pnpm install --frozen-lockfile && pnpm near:status`: contract ids,
     on-chain `code_hash == recorded`, live `get_config`, cert count.
   - `pnpm near:verify bound-agent2-mwfgi0ij.testnet`: a cert left Verified
     until 2026-11-30 (exit 0, "decision: deal").
   - `pnpm near:verify bound-agent-mwfgi0ij.testnet`: the demo agent after
     settlement (status Invalid, settlement numbers, exit 2).
   - `pnpm near:evidence`: re-checks every tx hash in `near/evidence/near-demo-*.json`
     against an archival RPC node (regular nodes forget transactions after a
     few epochs) and prints OK per step.
   - `pnpm near:mcp-smoke`: connects an MCP client to the `bound-near` server
     in-process and calls every read tool against testnet (8 checks, exit 0).
4. Replay the demo on testnet (optional): `pnpm near:demo`
   - creates fresh actors with the public faucet, runs acts 1-8, asserts every
     balance delta, writes new evidence. Needs the arbiter key of the canonical
     deployment. Without any key: `pnpm near:demo --fresh` deploys a private
     copy from a faucet account (that account is the arbiter). The faucet
     rate-limits per IP (we hit 429 after about 11 accounts); then the script
     needs `--funder <account>` with a local near-cli key.

5. Agents over MCP: `pnpm near:mcp` runs the `bound-near` MCP server on stdio
   with four tools: `near_verify_agent`, `near_get_certificate`,
   `near_check_agent_keys` (read-only, no key) and `near_pay`, enabled only
   when `BOUND_NEAR_AGENT_ID` and `BOUND_NEAR_AGENT_KEY` hold an agent account
   and its pay-only function-call key. Example client config:
   `{"command": "pnpm", "args": ["--dir", "<repo>", "near:mcp"]}`.
   - `evidence/mcp-agent-mwfgqr0u.txt`: `pnpm near:agent-mcp` confined cert 6's
     agent (`bound-agent2-mwfgqr0u.testnet`) to a pay-only key, then through
     the MCP tools paid 1 to the allowlisted service (tx), was refused
     `FloatExceeded` for 500 and `RecipientNotAllowed` for the treasury (txs),
     and its float dropped by exactly 1. Replaying needs the run's local demo
     keys (`near/.demo-keys/`, gitignored).
6. Web, read-only, no wallet: bound-web branch `near-hackathon`, page
   `/app/near` (verify box, certificate list, per-certificate key
   confinement, claims and settlement waterfall, demo runs re-checked against
   an archival node) and `GET /api/near/verify/<agent>`.

## Deployed on testnet

- Contract: `bound-v1.boundproto-nh26.testnet`
- Token: `usdc.boundproto-nh26.testnet` (mock USDC, open mint)
- Treasury / arbiter: `treasury.boundproto-nh26.testnet` /
  `arbiter.boundproto-nh26.testnet`
- Deploy and demo transactions with NearBlocks links:
  `deployments/testnet.json` and `evidence/near-demo-mwfgi0ij.txt`.

Without a browser: `near contract call-function as-read-only
bound-v1.boundproto-nh26.testnet get_config json-args '{}' network-config
testnet now`, or `pnpm near:status`.

## The demo, act by act (run mwfgi0ij)

1. Actors created; mock USDC minted; actors register storage on Bound.
2. Auditor stakes 600 (`ft_transfer_call` with `{"stake":{}}`). Operator
   publishes cert: bound 150, reserve 100, float cap 20, allowlist [service],
   premium 3. Agent accepts; reserve and float funded; auditor attests 200.
   Premium split: auditor +2.7, treasury +0.3.
3. Agent adds a function-call key (receiver bound-v1, method `pay`) and deletes
   its full-access key; `view_access_key_list` shows exactly that one key.
   Gate: an uncertified agent gets 403; the agent pays 5 through `pay` and is
   served; replaying the same tx hash gets 409; a 120 job is served on credit.
4. Attacks: pay 50 over a float of 15 (`FloatExceeded`, on chain); pay 10 to an
   attacker (`RecipientNotAllowed`, on chain); the same key calling
   `ft_transfer`, `mint` on the token, `withdraw`, or sending NEAR is refused
   by NEAR before execution (no transaction); operator halts, next pay
   `CertHalted`.
5. False `InsufficientReserve` claim: rejected in the same tx, bond 10 to the
   treasury.
6. Service files Harm 120 citing its unpaid invoice; early close refused
   (`WindowOpen`); arbiter upholds 120; pay refused (`CertFrozen`).
7. After 90 s anyone calls `close_window`: payable 120, victim +100 from the
   reserve, fee 0 (reserve exhausted), auditor slashed 20 to the treasury, 180
   back to the auditor, cert Invalid. Service withdraws 115 and its token
   balance rises by 115. The service lost 120 and recovered 100: victims are
   covered up to the reserve.
8. A second cert (agent2, expires 2026-11-30) is left Verified for judging.

## Libraries

near-sdk 5.30.1, near-contract-standards 5.30.1, near-sandbox 0.4, near-api
0.8, cargo-near-build 0.11, tokio, testresult (Rust); near-api-js 7.5.3,
@modelcontextprotocol/sdk 1.29, zod 3, tsx, vitest, typescript (TS). Tooling: cargo-near 0.23.0, near-cli-rs 0.31.1.

## AI disclosure

The NEAR contracts, tests, TypeScript package, scripts and these docs on this
branch were written by Claude Code (Anthropic) under the author's direction.
