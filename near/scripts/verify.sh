#!/usr/bin/env bash
# Build, test and check the NEAR integration. Exits non-zero on any failure.
#
#   near/scripts/verify.sh            build + fmt + clippy + unit + near-sandbox tests,
#                                     TS typecheck + vitest, wasm sha256 == deployed
#                                     code hash, and zero-secret testnet reads
#                                     (near:status, near:evidence)
#   near/scripts/verify.sh --offline  everything that needs no network
#                                     (the sandbox binary must already be cached)
#   near/scripts/verify.sh --demo     additionally replay the full demo on testnet
#                                     (creates fresh actors; needs the faucet or a
#                                     funder key, and the arbiter key; see README)
set -euo pipefail

OFFLINE=0
DEMO=0
for a in "$@"; do
  case "$a" in
    --offline) OFFLINE=1 ;;
    --demo) DEMO=1 ;;
    *) echo "unknown flag: $a" >&2; exit 2 ;;
  esac
done

NEAR_DIR="$(cd "$(dirname "$0")/.." && pwd)"
REPO="$(cd "$NEAR_DIR/.." && pwd)"
cd "$NEAR_DIR"

step() { printf '\n== %s\n' "$*"; }

step "build wasm with cargo near (mock-usdc, bound)"
(cd contracts/mock-usdc && cargo near build non-reproducible-wasm)
(cd contracts/bound && cargo near build non-reproducible-wasm)
ls -l target/near/bound/bound.wasm target/near/mock_usdc/mock_usdc.wasm

step "cargo fmt --check, clippy -D warnings"
cargo fmt --check
cargo clippy --all-targets -- -D warnings

step "cargo test: unit tests + near-sandbox integration tests"
cargo test

step "TypeScript: typecheck + vitest (@bound/near)"
(cd "$REPO/packages/near" && pnpm run typecheck && pnpm run test)

step "built wasm sha256 == sha256 recorded for the testnet deployment"
node -e '
const { createHash } = require("crypto");
const fs = require("fs");
const d = JSON.parse(fs.readFileSync("deployments/testnet.json", "utf8"));
let bad = 0;
for (const [name, file] of [["bound", "target/near/bound/bound.wasm"], ["token", "target/near/mock_usdc/mock_usdc.wasm"]]) {
  const sha = createHash("sha256").update(fs.readFileSync(file)).digest("hex");
  const want = d.contracts[name].wasmSha256;
  const ok = sha === want;
  if (!ok) bad++;
  console.log(`${ok ? "OK  " : "FAIL"} ${name}: built ${sha} ${ok ? "==" : "!="} deployed ${want}`);
}
process.exit(bad ? 1 : 0);
'

if [ "$OFFLINE" = 0 ]; then
  step "zero-secret testnet reads: near:status (on-chain code_hash, config)"
  (cd "$REPO" && pnpm --silent near:status)
  step "zero-secret testnet reads: near:evidence (every committed tx hash vs RPC)"
  (cd "$REPO" && pnpm --silent near:evidence)
fi

if [ "$DEMO" = 1 ]; then
  step "replay the demo on testnet (near:demo)"
  (cd "$REPO" && pnpm near:demo)
fi

step "all checks passed"
