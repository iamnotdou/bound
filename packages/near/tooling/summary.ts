// Full deployment record -> the public summary bundled in the npm package.

import type { TestnetDeployment } from "../src/testnet.js";
import type { NearDeployment } from "./deployment.js";

export function summarize(d: NearDeployment): TestnetDeployment {
  const run = d.demoRuns.at(-1);
  if (!run) throw new Error("deployment record has no demo run");
  return {
    network: "testnet",
    deployedAt: d.deployedAt,
    contracts: {
      bound: { accountId: d.contracts.bound.accountId, codeHash: d.contracts.bound.codeHash },
      token: {
        accountId: d.contracts.token.accountId,
        codeHash: d.contracts.token.codeHash,
        decimals: 6,
        note: d.contracts.token.note,
      },
    },
    accounts: d.accounts,
    rpc: d.rpc,
    demo: {
      runId: run.runId,
      verifiedAgent: run.agent2,
      verifiedCertId: run.cert2Id,
      settledAgent: run.agent,
      settledCertId: run.certId,
    },
  };
}
