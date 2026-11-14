// The public NEAR testnet deployment of Bound, bundled with the package as
// data/testnet.json (contract and token ids, code hashes, RPC endpoints and
// the two demo agents). Read once at import; no network, no keys.

import { readFileSync } from "node:fs";

export interface TestnetDeployment {
  network: "testnet";
  deployedAt: string;
  contracts: {
    bound: { accountId: string; codeHash: string };
    token: { accountId: string; codeHash: string; decimals: number; note: string };
  };
  accounts: { treasury: string; arbiter: string; owner: string };
  rpc: string[];
  /** The recorded demo run: one agent still Verified, one settled. */
  demo: {
    runId: string;
    verifiedAgent: string;
    verifiedCertId: number;
    settledAgent: string;
    settledCertId: number;
  };
}

// src/ (tsx, tests) and dist/ (published) both sit one level below data/.
export const TESTNET: TestnetDeployment = JSON.parse(
  readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8"),
) as TestnetDeployment;
