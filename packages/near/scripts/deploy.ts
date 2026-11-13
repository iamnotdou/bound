// pnpm near:deploy -- deploys mock USDC + Bound to NEAR testnet as sub-accounts
// of the deployer (default boundproto-nh26.testnet), signing with the
// deployer's near-cli legacy keychain credential. TESTNET ONLY.
// Usage: pnpm near:deploy [--deployer <id>] [--bound-name bound-v1]

import { existsSync } from "node:fs";
import { accountFromKey, testnetProvider } from "../src/client.js";
import { DEPLOYMENT_PATH, readCredential, saveDeployment } from "../src/deployment.js";
import { WASM, deployAll } from "../src/deploy-lib.js";

function arg(name: string, def: string): string {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : def;
}

async function main() {
  const deployer = arg("deployer", "boundproto-nh26.testnet");
  if (!deployer.endsWith(".testnet")) throw new Error("testnet accounts only");
  for (const w of Object.values(WASM)) {
    if (!existsSync(w))
      throw new Error(`missing ${w}; run near/scripts/verify.sh (or cargo near build) first`);
  }
  if (existsSync(DEPLOYMENT_PATH) && !process.argv.includes("--force")) {
    throw new Error(`${DEPLOYMENT_PATH} exists; pass --force to deploy a new set`);
  }
  const provider = testnetProvider();
  const parent = accountFromKey(provider, deployer, readCredential(deployer));
  const d = await deployAll(provider, parent, { boundName: arg("bound-name", "bound-v1") });
  saveDeployment(d);
  console.log(`wrote ${DEPLOYMENT_PATH}`);
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
