// Committed deployment data — the public, versioned record of where the
// contracts live. This is the source of truth for addresses; environment
// variables only hold credentials.
//
// IMPORTANT: use a static map, never `import(\`../deployments/${network}.json\`)`.
// Dynamic imports cannot be statically analysed, so bundlers omit the file and
// the browser build breaks. One network today makes the map trivial.
//
// Moves into packages/sdk at plan step 4.3.
import testnet from "../../deployments/testnet.json";

export type NetworkName = "testnet";

export interface Deployment {
  network: NetworkName;
  networkPassphrase: string;
  rpcUrl: string;
  horizonUrl: string;
  /** ISO-8601 timestamp of the deploy that produced these addresses. */
  deployedAt: string;
  /** Full commit SHA of the code that was deployed. */
  deployCommit: string;
  contracts: {
    registry: string;
    reserveVault: string;
    auditorStaking: string;
    feeEscrow: string;
    challengeManager: string;
    usdc: string;
  };
}

const DEPLOYMENTS: Record<NetworkName, Deployment> = {
  testnet: testnet as Deployment,
};

/**
 * Look up a committed deployment. `network` is optional and defaults to
 * `testnet` — a required env var for a choice with one option is an onboarding
 * tax for no benefit. Make it required the day mainnet exists.
 */
export function getDeployment(network: NetworkName = "testnet"): Deployment {
  const deployment = DEPLOYMENTS[network];
  if (!deployment) {
    throw new Error(`unknown network: ${network}`);
  }
  return deployment;
}

export function listNetworks(): NetworkName[] {
  return Object.keys(DEPLOYMENTS) as NetworkName[];
}
