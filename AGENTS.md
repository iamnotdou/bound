# bound — the protocol

Soroban contracts on Stellar, plus the two npm packages that are the only
sanctioned way to talk to them. No frontend lives here; that is `bound-web`.

| Path            | What it is                                                                                                                 |
| --------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `contracts/`    | Rust workspace. 7 deployed contracts, `spend-probe` (never deployed), `integration-tests` (offline cross-contract harness) |
| `bindings/`     | **Generated** TypeScript clients, one per deployed contract + the USDC SAC                                                 |
| `packages/sdk/` | `@bound/sdk` — published. `bound-client.ts` is the facade; `deployments.ts` is the address record                          |
| `packages/mcp/` | `@bound/mcp` — published. `tools.ts` is the **one** definition of every agent tool                                         |
| `scripts/`      | setup / deploy / demo / evidence / anchor, and the `*-smoke.ts` live suites                                                |
| `deployments/`  | `testnet.json` — committed, and the source of truth for every address                                                      |
| `docs/`         | Design, threat model, security review. Prose, not load-bearing for a build                                                 |

## Definition of done

```bash
pnpm verify   # typecheck, lint, format, vitest, cargo fmt/clippy/test
```

Offline, no credentials. It passes on a fresh clone with no `.env.testnet`, which
is the whole contract between local work and CI. `verify` skips `pnpm build` to
stay fast; CI runs both, so run `pnpm build` yourself before pushing a change to
either package.

`pnpm test` runs vitest against SDK **source** (aliased in `vitest.config.mts`),
so the unit suite needs no build and a failure points at an editable line.

## Costly commands

`deploy`, `setup`, `demo`, `demo:settle`, `demo:expiry`, `evidence`, `test:e2e`
and the four `*-smoke` scripts hit live testnet: they spend funds and mutate
on-chain state. Run one only when asked for it by name.

`pnpm deploy` is the sharpest: it mints new contract addresses, which invalidates
every address committed under `bindings/*/src/index.ts`. Deployment is the
maintainer's deliberate act, never a step in an edit loop.

## Guardrails

- **Secrets stay unprinted.** `.env.testnet` holds five live Stellar `S...` keys
  and `ANTHROPIC_API_KEY`; this repo is public. Compare hashes, never values.
  Commit `.env.example` and nothing else matching `.env*`.
- **Regenerate `bindings/`, never hand-edit it.** A hand edit is discarded on the
  next regeneration. There is deliberately **no `build:bindings` script**: the
  committed bindings were generated against a live deployment (`--contract-id`),
  which is what populates their `networks` block. Regenerating from local wasm
  produces no `networks` block and a different method-options type, so it would
  not be a faithful regeneration. Resolve that before adding one.
- **`deployments/testnet.json` belongs to `scripts/deploy-all.ts`.** Wrong shape
  → fix `serializeDeployment` in `packages/sdk/src/deployments.ts`, not the JSON.
- **A `pub fn` inside `#[contractimpl]` is the on-chain ABI.** Changing its
  signature changes the generated bindings and forces a redeploy to a new
  address. That is why `#![allow(clippy::too_many_arguments)]` sits at crate
  level in `registry` and `challenge-manager` rather than on the impl block.
- **`@bound/sdk` is built by tsup, not tsc.** It imports the bindings by
  relative path and tsup inlines them (`noExternal`); plain `tsc` would emit
  those relative specifiers into `dist/` and publish a package importing files
  outside its own tarball. Read `packages/sdk/tsup.config.ts` before touching
  that build. Its `BINDINGS` list names six of the eight — belt-and-braces,
  since esbuild inlines relative imports unconditionally; extend it only if the
  imports ever become bare names.

## Where a fact lives exactly once

- **Addresses, endpoints, actor public keys** → `deployments/testnet.json`, read
  through `getDeployment()` and published as `@bound/sdk/deployments`. tsup
  inlines the JSON, so the tarball carries the addresses. `bindings/*/src/index.ts`
  also carries a `networks.testnet.contractId` per package: generated, committed,
  and read by nothing. Keep it in mind when addresses change.
- **Credentials** → the environment only. `packages/sdk/src/config.ts` never
  reads a secret; `packages/mcp/src/accounts.ts` reads the five `*_SECRET`
  variables lazily so a read-only session touches none.
- **Agent tools** → `packages/mcp/src/tools.ts`. The `bound-mcp` executable and
  any AI-SDK loop are two adapters over one table; a consumer imports
  `@bound/mcp/tools` so a bundle never pulls the MCP server in. The package
  cannot import from an app — that is what made the old in-app server
  unpublishable — so it carries its own `accounts.ts` over the same env names.
- **Storage TTL reasoning** → the constants in `contracts/registry/src/lib.rs`.
  Every other contract repeats the two constants and points back there.

## Protocol semantics that prevent real bugs

`PaymentRouter::spent(cert_id)` is **gross routed flow, not loss**. The
arithmetic cannot be forged, so `spent > bound` is a sound _predicate_ and a
worthless _settlement rule_: one dollar shuttled between two addresses the same
operator controls drives the counter past any bound for the price of gas.
`contracts/spend-probe` is the executable proof, and it is kept in the workspace
for that reason. Anything that pays out sizes the payout by harm proven to a
party outside the operator's control, capped by the collateral actually behind
the certificate.

`PaymentRouter::transfer` may make **no sub-invocation** — x402 facilitator
settlement requires exactly one transfer event and no nesting. That is why the
router custodies USDC and moves internal balances, and why `enroll` copies the
certificate, its `expires_at` and its halt flag into router storage.

Reserves and auditor allocations unlock at `expires_at + CHALLENGE_WINDOW_SECONDS`
(the _settlement deadline_), not at expiry: a proof about post-expiry conduct is
only provable after expiry, so unlocking earlier would settle it against an
empty pot every time.

## Toolchain

Pinned so CI and your machine agree; `pnpm install` and `rustup` pick these up.

| Tool    | Pin     | Source                       |
| ------- | ------- | ---------------------------- |
| node    | 22.14.0 | `.nvmrc`                     |
| pnpm    | 10.27.0 | `packageManager`             |
| rust    | 1.97.1  | `rust-toolchain.toml`        |
| stellar | —       | not pinned; install manually |

`pnpm build:contracts` is `stellar contract build`; `rust-toolchain.toml`
installs both `wasm32v1-none` (the Soroban target) and `wasm32-unknown-unknown`
(what `cargo test` and clippy build for in places).

`pnpm-workspace.yaml` globs `packages/*` and `bindings/*`. The bindings are
workspace members but are imported **by relative path**, never by package name:
the Stellar CLI names them `registry` and `usdc`, which are real, unrelated
packages on npm.

## Contributing

Branch from `main` (`feat/`, `fix/`, `chore/`, `docs/`). Conventional commits,
enforced by a `commit-msg` hook. **The PR title is what lands** — CI squash-merges,
so write it as the sentence a stranger should read in six months; `pr-title` lints
it against the same commitlint config. `pre-commit` runs prettier on staged files
only: hooks are a convenience, CI is the gate.

## Connections

- `bound-web` consumes `@bound/sdk` **from npm**, at a published version. An SDK
  change reaches the app only after a version bump and publish (or a deliberate
  local link).
- `bound-docs` consumes `@bound/sdk/deployments` the same way, and renders every
  address from it. Publishing a new deployment is how the docs update.
- Nothing outside this repo may hold a contract address of its own.
