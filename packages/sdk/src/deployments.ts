// Committed deployment data — the public, versioned record of where the
// contracts live and who the demo actors are. Environment variables only
// hold credentials (*_SECRET, ANTHROPIC_API_KEY).
//
// IMPORTANT: use a static map, never `import(\`../deployments/${network}.json\`)`.
// Dynamic imports cannot be statically analysed, so bundlers omit the file and
// the browser build breaks. One network today makes the map trivial.
//
// Safe to import from client code — there are no secrets here. Published as the
// `@bound/sdk/deployments` subpath so browser bundles can take it without
// pulling in the chain client. The JSON is inlined by the bundler at build time,
// so the published package carries the addresses rather than reading them off
// disk.
import testnet from "../../../deployments/testnet.json";

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
  /**
   * Public key of a funded account the RPC may use as the source of
   * read-only simulations (sequence number). Not a secret — G..., not S....
   * Same as `accounts.operator` for the current demo deployment.
   */
  readSource: string;
  /** Demo actor public keys (G...). Secrets for these live only in env. */
  accounts: {
    operator: string;
    agent: string;
    auditor: string;
    challenger: string;
    counterparty: string;
  };
  contracts: {
    registry: string;
    reserveVault: string;
    auditorStaking: string;
    feeEscrow: string;
    challengeManager: string;
    usdc: string;
    /**
     * The PaymentRouter (SEP-41 wrapped USDC that meters spend per
     * certificate). Optional because the v1 deployment predates it — a
     * deployment record without this key is a v1 record, and the two new
     * trustless proofs are unavailable against it.
     */
    paymentRouter?: string;
    /**
     * The PremiumVault (coverage premiums, priced on bound x duration, accruing
     * to the auditor as yield). Optional for the same reason as paymentRouter:
     * a record without it is a deployment that predates the premium economy.
     */
    premiumVault?: string;
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

/**
 * Pure serialiser used by the deploy script. Addresses in, canonical JSON
 * string out — no filesystem, no network, no `Date.now()`. Callers supply
 * provenance so the function is deterministic and unit-testable.
 */
export function serializeDeployment(d: Deployment): string {
  const body: Deployment = {
    network: d.network,
    networkPassphrase: d.networkPassphrase,
    rpcUrl: d.rpcUrl,
    horizonUrl: d.horizonUrl,
    deployedAt: d.deployedAt,
    deployCommit: d.deployCommit,
    readSource: d.readSource,
    accounts: {
      operator: d.accounts.operator,
      agent: d.accounts.agent,
      auditor: d.accounts.auditor,
      challenger: d.accounts.challenger,
      counterparty: d.accounts.counterparty,
    },
    contracts: {
      registry: d.contracts.registry,
      reserveVault: d.contracts.reserveVault,
      auditorStaking: d.contracts.auditorStaking,
      feeEscrow: d.contracts.feeEscrow,
      challengeManager: d.contracts.challengeManager,
      usdc: d.contracts.usdc,
      ...(d.contracts.paymentRouter ? { paymentRouter: d.contracts.paymentRouter } : {}),
      ...(d.contracts.premiumVault ? { premiumVault: d.contracts.premiumVault } : {}),
    },
  };
  return `${JSON.stringify(body, null, 2)}\n`;
}
