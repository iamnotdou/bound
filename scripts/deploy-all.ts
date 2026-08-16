// Build, deploy, and wire up all 5 Bound Protocol contracts to Stellar testnet.
//
//   pnpm deploy   (→ ts-node --project scripts/tsconfig.json scripts/deploy-all.ts)
//
// Dependency note: the contracts reference each other (ReserveVault, AuditorStaking
// and Registry all need the ChallengeManager address; ChallengeManager needs all
// the others). `initialize` only stores addresses — it makes no cross-contract
// calls — so we deploy all 5 first, then initialize once every address is known.
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { existsSync, writeFileSync } from "node:fs";
import { readEnv, writeEnvValues, deploy, initialize, usdc, NETWORK } from "./lib";
import { serializeDeployment } from "../app/lib/deployments";

const ROOT = resolve(__dirname, "..");
const WASM_DIR = resolve(ROOT, "target", "wasm32-unknown-unknown", "release");
const DEPLOYMENTS_DIR = resolve(ROOT, "deployments");

/** Current HEAD — recorded as deploy provenance in deployments/<network>.json. */
function gitHead(): string {
  return execFileSync("git", ["rev-parse", "HEAD"], {
    cwd: ROOT,
    encoding: "utf8",
  }).trim();
}

// contract → wasm filename (cargo replaces '-' with '_')
const WASM = {
  reserve_vault: "reserve_vault.wasm",
  auditor_staking: "auditor_staking.wasm",
  fee_escrow: "fee_escrow.wasm",
  challenge_manager: "challenge_manager.wasm",
  registry: "registry.wasm",
} as const;

// Economic parameters
const AUDITOR_MIN_STAKE = usdc(500); // min stake to be a registered auditor
const CHALLENGE_MIN_BOND = usdc(100); // min bond to open a challenge
const RESERVE_LOCK_SECONDS = 365 * 24 * 3600; // reserve locked ~1 year out

function buildWasm(): void {
  console.log("Building contracts (release wasm)…");
  execFileSync("cargo", ["build", "--release", "--target", "wasm32-unknown-unknown"], {
    cwd: ROOT,
    stdio: "inherit",
  });
}

function wasmPath(file: string): string {
  const p = resolve(WASM_DIR, file);
  if (!existsSync(p)) throw new Error(`wasm not found: ${p} — did the build succeed?`);
  return p;
}

function main() {
  const env = readEnv();
  const operatorSecret = env.OPERATOR_SECRET;
  const operatorAddr = env.OPERATOR_ADDRESS;
  const usdcAddr = env.USDC_ADDRESS;

  for (const [k, v] of Object.entries({
    OPERATOR_SECRET: operatorSecret,
    OPERATOR_ADDRESS: operatorAddr,
    USDC_ADDRESS: usdcAddr,
  })) {
    if (!v) throw new Error(`missing ${k} in .env.testnet — run \`pnpm setup\` first`);
  }

  buildWasm();

  // --- Phase A: deploy all 5, collect addresses ---
  console.log("\nDeploying contracts…");
  const addr = {
    reserve_vault: deploy(wasmPath(WASM.reserve_vault), operatorSecret),
    auditor_staking: deploy(wasmPath(WASM.auditor_staking), operatorSecret),
    fee_escrow: deploy(wasmPath(WASM.fee_escrow), operatorSecret),
    challenge_manager: deploy(wasmPath(WASM.challenge_manager), operatorSecret),
    registry: deploy(wasmPath(WASM.registry), operatorSecret),
  };
  for (const [name, id] of Object.entries(addr)) console.log(`  ${name.padEnd(18)} ${id}`);

  // Persist immediately so a failed init step doesn't lose deployed addresses.
  writeEnvValues({
    RESERVE_VAULT_ADDRESS: addr.reserve_vault,
    AUDITOR_STAKING_ADDRESS: addr.auditor_staking,
    FEE_ESCROW_ADDRESS: addr.fee_escrow,
    CHALLENGE_MANAGER_ADDRESS: addr.challenge_manager,
    REGISTRY_ADDRESS: addr.registry,
  });

  // Output of record: committed deployment data. .env.testnet still gets the
  // same addresses (above) for local secrets workflows; this file is what the
  // app and the SDK will read after step 3.3.
  const deploymentPath = resolve(DEPLOYMENTS_DIR, `${NETWORK}.json`);
  writeFileSync(
    deploymentPath,
    serializeDeployment({
      network: "testnet",
      networkPassphrase: env.STELLAR_NETWORK_PASSPHRASE ?? "Test SDF Network ; September 2015",
      rpcUrl: env.STELLAR_RPC_URL ?? "https://soroban-testnet.stellar.org",
      horizonUrl: env.STELLAR_HORIZON_URL ?? "https://horizon-testnet.stellar.org",
      deployedAt: new Date().toISOString(),
      deployCommit: gitHead(),
      // Public G... key used as the RPC simulation source — not a secret.
      readSource: operatorAddr,
      // Demo actor public keys (G...). Secrets never leave .env.testnet.
      accounts: {
        operator: operatorAddr,
        agent: env.AGENT_ADDRESS,
        auditor: env.AUDITOR_ADDRESS,
        challenger: env.CHALLENGER_ADDRESS,
        counterparty: env.COUNTERPARTY_ADDRESS,
      },
      contracts: {
        registry: addr.registry,
        reserveVault: addr.reserve_vault,
        auditorStaking: addr.auditor_staking,
        feeEscrow: addr.fee_escrow,
        challengeManager: addr.challenge_manager,
        usdc: usdcAddr,
      },
    }),
  );
  console.log(`  wrote ${deploymentPath}`);

  // --- Phase B: initialize (all addresses known) ---
  const unlockAt = String(Math.floor(Date.now() / 1000) + RESERVE_LOCK_SECONDS);
  // Arbiter resolves only the subjective challenge paths (BoundExceeded /
  // FakeSignature); the headline InsufficientReserve path is trustless. For the
  // demo the operator stands in as arbiter.
  const arbiter = operatorAddr;

  console.log("\nInitializing contracts…");

  console.log("  reserve_vault");
  initialize(addr.reserve_vault, operatorSecret, [
    "--operator",
    operatorAddr,
    "--challenge_manager",
    addr.challenge_manager,
    "--token",
    usdcAddr,
    "--unlock_at",
    unlockAt,
  ]);

  console.log("  auditor_staking");
  initialize(addr.auditor_staking, operatorSecret, [
    "--challenge_manager",
    addr.challenge_manager,
    "--registry",
    addr.registry,
    "--token",
    usdcAddr,
    "--min_stake",
    AUDITOR_MIN_STAKE,
  ]);

  console.log("  fee_escrow");
  initialize(addr.fee_escrow, operatorSecret, [
    "--challenge_manager",
    addr.challenge_manager,
    "--token",
    usdcAddr,
  ]);

  console.log("  challenge_manager");
  initialize(addr.challenge_manager, operatorSecret, [
    "--registry",
    addr.registry,
    "--auditor_staking",
    addr.auditor_staking,
    "--reserve_vault",
    addr.reserve_vault,
    "--fee_escrow",
    addr.fee_escrow,
    "--token",
    usdcAddr,
    "--arbiter",
    arbiter,
    "--min_stake",
    CHALLENGE_MIN_BOND,
  ]);

  console.log("  registry");
  initialize(addr.registry, operatorSecret, [
    "--challenge_manager",
    addr.challenge_manager,
    "--auditor_staking",
    addr.auditor_staking,
  ]);

  console.log(
    `\n✓ All 5 contracts deployed, initialized, written to .env.testnet and deployments/${NETWORK}.json`,
  );
}

main();
