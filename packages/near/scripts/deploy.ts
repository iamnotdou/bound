// pnpm near:deploy -- deploys mock USDC + Bound to NEAR testnet as sub-accounts
// of the deployer (default boundproto-nh26.testnet), signing with the
// deployer's near-cli legacy keychain credential. TESTNET ONLY.
// Usage: pnpm near:deploy [--deployer <id>] [--bound-name bound-v1]
//        pnpm near:deploy --redeploy --bound-name bound-v2 --reason "..."
//          deploys only a new Bound contract next to the existing token,
//          treasury and arbiter, and moves the old contract (with its demo
//          runs) into `previous` in near/deployments/testnet.json.

import { existsSync } from "node:fs";
import { accountFromKey, testnetProvider } from "../src/client.js";
import {
  DEPLOYMENT_PATH,
  loadDeployment,
  readCredential,
  saveDeployment,
} from "../src/deployment.js";
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
  const redeploy = process.argv.includes("--redeploy");
  if (existsSync(DEPLOYMENT_PATH) && !redeploy && !process.argv.includes("--force")) {
    throw new Error(`${DEPLOYMENT_PATH} exists; pass --force to deploy a new set`);
  }
  const provider = testnetProvider();
  const parent = accountFromKey(provider, deployer, readCredential(deployer));
  const old = redeploy ? loadDeployment() : null;
  const boundName = arg("bound-name", "bound-v1");
  if (old && old.contracts.bound.accountId.startsWith(`${boundName}.`))
    throw new Error(`--bound-name ${boundName} is the current contract; pick a new name`);
  const d = await deployAll(provider, parent, { boundName, reuseExisting: redeploy });
  if (old) {
    d.contracts.token = old.contracts.token;
    d.txs = [...old.txs.filter((t) => !/deploy bound|register bound/.test(t.step)), ...d.txs];
    d.previous = [
      {
        accountId: old.contracts.bound.accountId,
        codeHash: old.contracts.bound.codeHash,
        wasmSha256: old.contracts.bound.wasmSha256,
        deployedAt: old.deployedAt,
        supersededAt: d.deployedAt,
        reason: arg("reason", "superseded"),
        txs: old.txs.filter((t) => /deploy bound|register bound/.test(t.step)),
        demoRuns: old.demoRuns,
      },
      ...(old.previous ?? []),
    ];
  }
  saveDeployment(d);
  console.log(`wrote ${DEPLOYMENT_PATH}`);
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
