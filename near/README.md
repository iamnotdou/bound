# Bound on NEAR

Built at NEAR HACKATHON. This is the Bound protocol's NEAR integration.
Everything in `near/` and `packages/near/` was written in the hackathon window
on branch `near-hackathon`. The Soroban contracts in `contracts/` are prior
work. Code written in the window:
[`pre-hackathon...hackathon-submission`](https://github.com/iamnotdou/bound/compare/pre-hackathon...hackathon-submission).
Outside `near/` and `packages/near/` that diff touches only the root
`package.json` (`near:*` scripts), `Cargo.toml` (`exclude = ["near"]`),
`.prettierignore`, `pnpm-lock.yaml` and the NEAR section of the root
`README.md`.

**The bond-and-insurance layer for AI agents on NEAR.** You hire a contractor
because they are bonded and insured, not because you trust them personally.
Bound gives an AI agent the same backing, so a counterparty can check it before
dealing with the agent. The agent is bonded by a USDC reserve the operator
pre-funds and locks, and insured by an auditor's slashable stake locked to that
one certificate. A spend meter that the agent's NEAR key cannot get around
limits what it can send. It is a surety bond plus staked cover, not a regulated
insurance product. `verify(agent)` returns
`coverage = reserve_locked + allocation`, the cover. That is the most that victims
upheld by the arbiter can recover in total. If the agent causes harm, a claim
window opens. One atomic settlement then pays the victim from the reserve
first and then from the auditor's slash, for whatever the reserve could not
cover.

**Trust model.** On testnet the author holds the arbiter, the treasury and the
contract's full-access key. The arbiter decides harm claims and could uphold a
fake one. Forfeited bonds and leftover slash go to the treasury. See
`DESIGN.md` section 7, which also has the roadmap (a per-certificate or
Sputnik DAO arbiter).

## Links

- Site: [near.boundprotocol.dev](https://near.boundprotocol.dev) (landing with live testnet numbers)
- Docs: [near.boundprotocol.dev/docs](https://near.boundprotocol.dev/docs) (quickstart, protocol, SDK, CLI, MCP, HTTP API, contract reference)
- Deck: [near.boundprotocol.dev/deck](https://near.boundprotocol.dev/deck)
- Web demo: [near.boundprotocol.dev/app/near](https://near.boundprotocol.dev/app/near)
- npm: [`@bound/near`](https://www.npmjs.com/package/@bound/near) (published right after submission prep):
  `npx -y @bound/near verify bound-agent2-mwfnc6q8.testnet`
- MCP: `{"command": "npx", "args": ["-y", "@bound/near", "mcp"]}`

## Layout

| Path                       | What                                                                                                                                                                                                             |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `contracts/bound`          | the protocol, one near-sdk 5.30.1 contract (`src/lib.rs`), settlement math (`src/settle.rs`), unit tests (`src/tests.rs`), near-sandbox tests (`tests/sandbox.rs`)                                               |
| `contracts/mock-usdc`      | NEP-141/145/148 token, 6 decimals, open `mint` (testnet play money)                                                                                                                                              |
| `deployments/testnet.json` | contract ids, deploy txs, wasm sha256 + on-chain code hash, demo runs with every tx hash, the superseded bound-v1 under `previous`                                                                               |
| `evidence/`                | committed demo logs: `near-demo-<run>.txt` (human) + `.json` (machine), gate ledger and invoices, MCP agent runs                                                                                                 |
| `scripts/verify.sh`        | install check, build, test, checks, optional demo replay                                                                                                                                                         |
| `DESIGN.md`                | Soroban to NEAR deviations, trust model, limits, test map, failure modes, bound-v1                                                                                                                               |
| `../packages/near`         | `@bound/near` (npm-ready, 0.1.0): read client, `bound-near` CLI (`verify`, `cert`, `mcp`) and MCP server in `src/`; deploy, demo and gate tooling (needs local keys, not published) in `tooling/` and `scripts/` |

## Try it

Prerequisites: Rust via rustup (the pinned 1.95.0 toolchain installs itself),
`cargo-near` 0.23, node 22 and pnpm 10. Run everything from the repo root.
First run:

```
pnpm install --frozen-lockfile
```

`verify.sh` also runs this install if `node_modules` is missing.

1. Full lifecycle offline, with no network and no keys: `cd near && cargo test`
   - You should see `test result: ok. 132 passed` (unit), then `2 passed`
     (near-sandbox: `full_lifecycle_settles_victim`,
     `withdraw_failure_recredits_and_guards_hold`), then `4 passed` (mock
     token). The first run downloads the sandbox binary.
2. Everything at once: `near/scripts/verify.sh`
   - This builds both wasm files, runs fmt and clippy, all tests, the TS
     typecheck and vitest. It checks that the built wasm sha256 equals the
     deployed one, then runs `near:status` and `near:evidence` (below). It ends
     with `== all checks passed` and exit 0.
3. Zero-secret testnet reads (no `~/.near-credentials` needed). Use
   `pnpm --silent` so pnpm does not add its own lines:
   - `pnpm --silent near:status` shows contract ids, the on-chain
     `code_hash == recorded`, the live `get_config` and the cert count.
   - `pnpm --silent near:verify bound-agent2-mwfnc6q8.testnet` shows cert 1,
     left Verified until 2026-11-30. It prints coverage 130 and
     `confinement: confined (1 key(s), no contract code, no global contract)`, then
     `decision: deal`, exit 0.
   - `pnpm --silent near:verify bound-agent-mwfnc6q8.testnet` shows the demo
     agent after settlement: status Invalid and the settlement numbers. It
     exits with code 2 on purpose (2 = "refuse"). Its `allocation` and
     `coverage` lines show the unslashed 180 still held by the cert: that is
     not live coverage. After the settlement deadline the auditor may call
     `release_allocation` to take it back, and then those lines drop. Without `--silent`, pnpm adds
     `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL` / `ELIFECYCLE`. That only reports the
     exit code 2 and is not a crash.
   - `pnpm --silent near:evidence` re-checks every recorded tx hash: the demo
     runs, the MCP agent runs and the deployment txs (of bound-v3 and of the
     superseded bound-v2 and bound-v1). It asks an archival RPC node,
     because regular nodes forget transactions after a few epochs, and prints
     OK per tx.
   - `pnpm --silent near:mcp-smoke` connects an MCP client to the `bound-near`
     server in-process and calls every read tool against testnet (8 checks,
     exit 0).
4. Replay the demo on testnet (optional): `pnpm near:demo`
   - It creates fresh actors, runs acts 1-8, asserts every balance delta and
     writes new evidence. On the canonical deployment it needs the arbiter key,
     which only the author has. Without any key, use `pnpm near:demo --fresh`:
     it creates a new deployer account and deploys a private copy from it. The
     arbiter of that copy is `arbiter.<deployer>`, a sub-account the script
     creates, with its key saved in your own `~/.near-credentials/testnet`, so
     you, not the author, decide its claims.
   - The public faucet rate-limits per IP, sometimes for hours. When it
     refuses, the script creates actors through the `testnet` registrar,
     funded by `--funder <account>`. The default funder is
     `boundproto-nh26.testnet`, the author's own account. A reviewer passes its
     own account, with a key in `~/.near-credentials/testnet` (about 10 testnet
     NEAR for `--fresh`). Without that key the script stops and says so.
5. Agents over MCP: `pnpm --silent near:mcp` runs the `bound-near` MCP server
   on stdio. It has four tools:
   - `near_verify_agent` (returns coverage)
   - `near_get_certificate`
   - `near_check_agent_keys` (pay-only keys AND no contract code, local or global)
   - `near_pay`, enabled only when `BOUND_NEAR_AGENT_ID` and
     `BOUND_NEAR_AGENT_KEY` hold an agent account and its pay-only
     function-call key.

   The first three are read-only and need no key. Client config (stdout
   stays pure JSON-RPC):
   `{"command": "pnpm", "args": ["--silent", "--dir", "<repo>", "near:mcp"]}`,
   or without a clone, from npm:
   `{"command": "npx", "args": ["-y", "@bound/near", "mcp"]}`.
   - `near/evidence/mcp-agent-mwfnc6q8.txt` is the record of `pnpm near:agent-mcp`.
     It confined cert 1's agent (`bound-agent2-mwfnc6q8.testnet`) to a pay-only
     key. Then, through the MCP tools, the agent paid 1 to the allowlisted
     service. It was refused with `FloatExceeded` for 500 and with
     `RecipientNotAllowed` for the treasury. Every write (the key swap and the
     three payments) has a tx; the reads are view calls. The tool
     reported it confined (code hash all ones), and its float dropped by
     exactly 1. Replaying needs the run's local demo keys
     (`near/.demo-keys/`, gitignored).
   - No language model runs in these scripts. The calls are the exact tool
     calls an LLM agent holding that key could make, so a prompt-injected model
     is refused in the same way.

6. Web, read-only, no wallet: bound-web branch `near-hackathon`, run
   `pnpm install && pnpm build && pnpm start`, then open `/app/near`. The page
   has a verify box, the certificate list, coverage and key plus code
   confinement per certificate, claims, the settlement waterfall, and the demo
   runs re-checked against an archival node. It also serves
   `GET /api/near/verify/<agent>`.

## Deployed on testnet

- Contract: `bound-v3.boundproto-nh26.testnet` (code hash
  `A3yf1yzihbUdZAaHn92aAm5dBAdFCq1QCwhBJixD2B4H`, wasm sha256
  `867c678c...`), built from this branch with the second review's fixes.
  The build is `cargo near build non-reproducible-wasm` (the same command
  `verify.sh` runs), not a reproducible Docker build: the check is that a
  local build of this commit gives the same wasm sha256 as the deployed code
  hash, on the pinned toolchain.
- Token: `usdc.boundproto-nh26.testnet` (mock USDC, open mint)
- Treasury / arbiter: `treasury.boundproto-nh26.testnet` /
  `arbiter.boundproto-nh26.testnet`
- Deploy and demo transactions with NearBlocks links are in
  `deployments/testnet.json` and `evidence/near-demo-mwfnc6q8.txt`.
  (NearBlocks blocks `curl` with a bot wall. Use a browser, or
  `pnpm --silent near:evidence`.)

Without a browser: `near contract call-function as-read-only
bound-v3.boundproto-nh26.testnet get_config json-args '{}' network-config
testnet now`, or `pnpm --silent near:status`.

**bound-v1 is superseded.** The first deployment,
`bound-v1.boundproto-nh26.testnet`, had a certificate-id aliasing bug that
was found in review and is fixed in bound-v2 (`DESIGN.md` section 9). Its runs
`mwfgi0ij` / `mwfgqr0u` and MCP run `mwfgqr0u` stay in `evidence/` and are
still checked by `near:evidence`. Its certificates 0-2 came from early
development runs and carry no evidence. Do not use bound-v1.

**bound-v2 is superseded.** `bound-v2.boundproto-nh26.testnet` ran the code
from before the second review's fixes (`DESIGN.md` section 10). In its run
`mwfiv11v` the unslashed allocation went back to the auditor's free stake at
`close_window` (free 580, allocated 0); on bound-v3 it stays allocated until
the settlement deadline. Its runs stay in `evidence/` and are still checked by
`near:evidence`. Do not use bound-v2.

## The demo, act by act (run mwfnc6q8, bound-v3)

1. Actors are created, mock USDC is minted, and the actors register storage on
   Bound.
2. The auditor stakes 600 (`ft_transfer_call` with `{"stake":{}}`). The
   operator publishes cert 0: bound 150, reserve 100, float cap 20, allowlist
   [service], premium 3. The agent accepts, the reserve and float are funded,
   and the auditor attests 200. Premium split: auditor +2.7, treasury +0.3.
3. The agent adds a function-call key (receiver bound-v3, method `pay`) and
   deletes its full-access key. `view_access_key_list` shows exactly that one
   key, `view_account.code_hash` is `111...1` (no contract code) and the
   `global_contract_*` fields are null (no global contract). At the
   gate:
   - an uncertified agent gets 403;
   - the agent pays 5 through `pay` and is served;
   - replaying the same tx hash gets 409;
   - a 120 job is served on credit. The limit is `min(bound, coverage)` minus
     outstanding invoices.
4. Scripted attack, the calls a hijacked agent would make with its key:
   - pay 50 over a float of 15: refused on chain with `FloatExceeded`;
   - pay 10 to an attacker: refused on chain with `RecipientNotAllowed`;
   - the same key calling `ft_transfer` or `mint` on the token, calling
     `withdraw`, or sending NEAR: refused by NEAR before execution, so no
     transaction exists;
   - the operator halts the cert, and the next pay fails with `CertHalted`.
5. A false `BoundExceeded` claim (spent 5 <= bound 150) is rejected in the
   same tx, and its bond of 10 goes to the treasury.
6. The service files Harm 120, citing its unpaid invoice. An early close is
   refused (`WindowOpen`). The arbiter upholds 120. A pay attempt is refused
   (`CertFrozen`).
7. After 90 s anyone calls `close_window`. Payable is 120. The victim gets 120:
   100 from the reserve and 20 from the auditor's slash. The challenger fee is
   0 because the reserve is exhausted, the treasury gets 0, and the cert is
   Invalid. The unslashed 180 stays allocated to the cert, still answering
   later Harm claims, and goes back to the auditor at the settlement deadline
   (expires_at + post_expiry) via `release_allocation`. The service withdraws 135 (5
   prepaid + 120 + 10 bond back) and its token balance rises by 135.
8. A second cert (cert 1, agent2, expires 2026-11-30) is left Verified for
   judging. The evidence log counts certs by ordinal and calls this one
   "cert 2" (`operator publishes cert 2 for agent2`); its on-chain id is 1,
   as `verify` on the next line of the log shows.

## Libraries

Rust: near-sdk 5.30.1, near-contract-standards 5.30.1, near-sandbox 0.4,
near-api 0.8, cargo-near-build 0.11, tokio, testresult, serde_json.
TypeScript: near-api-js 7.5.3, @modelcontextprotocol/sdk 1.29, zod 3, tsx,
vitest, typescript, @types/node. Tooling: cargo-near 0.23.0, near-cli-rs
0.31.1.

## AI disclosure

Claude Code (Anthropic) wrote the NEAR contracts, tests, TypeScript package,
scripts and these docs on this branch, under the author's direction. A
pre-submission review (also AI-run) found the bound-v1 issues fixed in
bound-v2, and a second one the issues fixed in bound-v3. They are listed in
`DESIGN.md`.
