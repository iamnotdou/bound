# Working in this repo

Bound Protocol: six Soroban contracts on Stellar, a TypeScript client over
generated bindings, and a Next.js app. This file is how work is done here. It
serves humans and agents equally, and stays honest because both read it.

## Definition of done

```bash
pnpm verify
```

Green means done. It runs typecheck, lint, formatting, unit tests, and the
contract fmt/clippy/test suites — about 6 seconds, **no network and no
credentials**. If it passes on a fresh clone with no `.env.testnet`, it passes
in CI. That is the whole contract between local work and CI.

The one gap: `verify` does not run `pnpm build`, to stay fast. CI runs both, so
run `pnpm build` yourself before pushing anything that touches the app shell.

## Commands

Run everything from the repo root.

| Command                        | Meaning                                                                   | Network | Secrets |
| ------------------------------ | ------------------------------------------------------------------------- | ------- | ------- |
| `pnpm install`                 | get ready — also installs git hooks                                       | once    | no      |
| `pnpm dev`                     | run the app locally                                                       | no      | no      |
| `pnpm build`                   | production build (SDK, then the app)                                      | no      | no      |
| `pnpm build:sdk`               | just `@bound/sdk` (tsup)                                                  | no      | no      |
| `pnpm verify`                  | **everything CI runs**                                                    | no      | no      |
| `pnpm typecheck`               | types only                                                                | no      | no      |
| `pnpm lint` / `lint:fix`       | eslint                                                                    | no      | no      |
| `pnpm format` / `format:check` | prettier                                                                  | no      | no      |
| `pnpm test` / `test:watch`     | unit tests                                                                | no      | no      |
| `pnpm test:contracts`          | `cargo test` — 81 tests, 6 contracts + the offline cross-contract harness | no      | no      |
| `pnpm lint:contracts`          | `cargo clippy -D warnings`                                                | no      | no      |
| `pnpm format:contracts`        | `cargo fmt --check`                                                       | no      | no      |
| `pnpm build:contracts`         | 7 wasm artifacts                                                          | no      | no      |
| `pnpm test:e2e`                | 5 live smoke suites                                                       | **yes** | **yes** |
| `pnpm demo`                    | 8-step end-to-end demo                                                    | **yes** | **yes** |
| `pnpm deploy`                  | deploy contracts                                                          | **yes** | **yes** |
| `pnpm setup`                   | create + fund testnet accounts                                            | **yes** | **yes** |

`pnpm build:contracts` must target `wasm32-unknown-unknown`. Do not switch it to
`stellar contract build`: the 27.x CLI defaults to `wasm32v1-none`, which fails
on this project — it is on soroban-sdk 22, and every deployed artefact is
`wasm32-unknown-unknown`.

## Never do these

Each is a rule because the consequence is expensive or irreversible.

- **Never run `pnpm deploy`.** It spends testnet funds and produces new contract
  addresses, which invalidates every address committed in `bindings/*/src/index.ts`.
  Deployment is a deliberate act by the maintainer, never part of an edit loop.
- **Never run `pnpm test:e2e`, `pnpm demo` or `pnpm setup` unless explicitly
  asked.** They mutate on-chain state and spend funds.
- **Never print, echo, log or commit a `*_SECRET` or `ANTHROPIC_API_KEY`.** The
  repo is public. `.env.testnet` holds five live Stellar secret keys. If you need
  to compare secrets, compare hashes.
- **Never edit `bindings/` by hand.** It is generated from the contract wasm. A
  hand edit is silently discarded the next time it is regenerated.
- **Never hand-edit `deployments/*.json`.** The deploy script owns that file —
  it is rewritten on every `pnpm deploy`. Edit the serialiser if the shape is
  wrong, not the JSON.
- **Never commit `.env*` except `.env.example`.**

## Map

| Path                                                | What lives there                                                                                                              |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `contracts/`                                        | 6 Soroban contracts (Rust workspace) + their tests, plus `integration-tests` (the offline cross-contract harness)             |
| `bindings/`                                         | **Generated** TypeScript clients — do not hand-edit                                                                           |
| `packages/sdk/`                                     | `@bound/sdk` — the publishable client: `bound-client.ts` (the facade over all 6 bindings), config, deployments, money helpers |
| `apps/dashboard/app/lib/`                           | App-only server code: secrets (`accounts.ts`), agent tools, tx building, UI config                                            |
| `apps/dashboard/app/api/`                           | 12 route handlers                                                                                                             |
| `apps/dashboard/app/`                               | Next.js app: chat, dashboard, control, auditor views                                                                          |
| `apps/dashboard/components/`, `apps/dashboard/lib/` | shadcn UI primitives and the `cn` helper                                                                                      |
| `mcp/`                                              | MCP server exposing the agent tools                                                                                           |
| `scripts/`                                          | setup, deploy, demo, dry-run, and the 5 `*-smoke.ts` suites                                                                   |
| `test/`                                             | vitest setup; the tests themselves sit next to their source                                                                   |
| `docs/`                                             | PROJECT (trust model + limitations), WRITEUP, VERIFY, RESOURCES                                                               |

## Where contract addresses actually live

**`deployments/testnet.json`** — committed, and the single source of truth.
Server (`packages/sdk/src/config.ts`) and browser
(`apps/dashboard/app/lib/ui-config.ts`) both read it through `getDeployment()`,
which the SDK publishes at `@bound/sdk/deployments`. tsup inlines the JSON, so
the published tarball carries the addresses rather than reading them off disk.
It holds network endpoints, the six contract ids, the five actor `G...` public
keys, and the RPC read-source account.

`.env.testnet` is **secrets only** (`*_SECRET`, `ANTHROPIC_API_KEY`) plus the
optional `STELLAR_NETWORK` selector. `pnpm build` and `pnpm verify` run with no
`.env.testnet` at all.

`bindings/*/src/index.ts` still carries a `networks.testnet.contractId` per
package — generated, committed, public, and **not** what the app reads. It is a
duplicate to keep in mind when addresses change.

## Workspace layout

`pnpm-workspace.yaml` globs `packages/*`, `apps/*`, and `bindings/*`. `apps/`
holds the Next.js app (`apps/dashboard`); `packages/` holds `@bound/sdk`, which
the app and the root `scripts/` both consume as `workspace:*`.

`@bound/sdk` is built by **tsup, not tsc**, and this is load-bearing. It reaches
the bindings by relative path (below) and nothing ever builds `bindings/*/dist`,
so a plain `tsc` would emit those same relative specifiers into `dist/` and
publish a package whose every import points outside its own tarball. tsup inlines
all six binding packages (`noExternal`) and leaves only real npm packages
(`@stellar/stellar-sdk`, `buffer`) external. Before touching that build, read
`packages/sdk/tsup.config.ts`.

Because the dashboard consumes the SDK's **built** output, `pnpm build` and
`pnpm typecheck` both run `pnpm build:sdk` first. `pnpm test` does not: vitest
aliases `@bound/sdk` to source, so the unit suite needs no build.

The six `bindings/*` packages are workspace members but are **imported by
relative path** (`../../bindings/registry/src`), never by package name. That is
deliberate: the Stellar CLI names them `registry`, `usdc`, `fee_escrow` and so
on, and `registry` and `usdc` are **real, unrelated packages on npm**. Adding
either as a named dependency would be ambiguous at best and fetch a stranger's
code at worst. `packages/sdk` keeps those path imports and bundles the bindings
into its own output, so a consumer of `@bound/sdk` never has to resolve them.

There is deliberately **no `build:bindings` script yet.** The committed bindings
were generated against a live deployment (`--contract-id`), which is what
populates that `networks` block; regenerating from a local wasm produces no
`networks` block and a different method-options type, so it would not be a
faithful regeneration. Do not add one without resolving that.

## Contributing

- Branch from `main`. Prefix: `feat/`, `fix/`, `chore/`, `docs/`.
- **Conventional commits**, enforced by a `commit-msg` hook. Allowed types:
  `feat, fix, chore, docs, refactor, test, ci, style, perf, build, revert`.
- **The PR title matters most.** We squash-merge, so the PR title becomes the
  single commit message on `main` and your individual messages are discarded.
  Write it as the sentence a stranger should read in six months.
- Small PRs. If it needs a table of contents, split it.

`pnpm install` installs two git hooks. You never invoke them directly:

| Hook       | Runs                                      | Budget  |
| ---------- | ----------------------------------------- | ------- |
| pre-commit | `lint-staged` — prettier on staged files  | ~1.1s   |
| commit-msg | `commitlint` — conventional commit format | instant |

Hooks are a convenience, not a gate — `--no-verify` bypasses them and a
contributor may never have run `pnpm install`. CI is the real enforcement. They
deliberately do not run eslint, typecheck, or tests: a slow hook trains people
to skip it, and then it protects nothing.

## Toolchain

Pinned, so CI and your machine agree. `pnpm install` and `rustup` pick these up
automatically.

| Tool    | Pin     | Source                        |
| ------- | ------- | ----------------------------- |
| node    | 22.14.0 | `.nvmrc`                      |
| pnpm    | 10.27.0 | `packageManager`              |
| rust    | 1.97.1  | `rust-toolchain.toml`         |
| stellar | 27.1.0  | not pinned — install manually |
