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
import testnetAnchor from "../../../deployments/testnet-anchor.json";

/**
 * Every network this package carries a deployment for.
 *
 * The single source of truth for the set. `DEPLOYMENTS` below is typed
 * `Record<NetworkName, Deployment>`, so adding a name here without adding the
 * record is a type error rather than a runtime surprise — which is the point:
 * a second deployment is three edits in this one file, not a type change
 * rippling through the SDK, the scripts and the app.
 *
 * A tuple rather than `keyof typeof DEPLOYMENTS` because `Deployment.network`
 * is itself a `NetworkName`, and deriving the union from the map would make the
 * two definitions circular.
 */
export const NETWORK_NAMES = ["testnet", "testnet-anchor"] as const;

export type NetworkName = (typeof NETWORK_NAMES)[number];

/**
 * What every caller gets when it does not say. Bound is testnet-only today, so
 * a required selector for a choice with one option would be an onboarding tax;
 * the day a second network exists, this is the one line that decides the
 * default.
 */
export const DEFAULT_NETWORK: NetworkName = "testnet";

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
  /**
   * The classic issuer (G...) of the asset behind `contracts.usdc`.
   *
   * Stated rather than derived, because the obvious derivation is wrong on any
   * deployment that matters: `accounts.operator` issues the mock token and
   * nothing else. An anchor-denominated deployment holds money somebody else
   * issues, and an app that assumes otherwise looks up balances under an
   * issuer no trustline names — silently reading zero for a funded wallet.
   */
  usdcIssuer: string;
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
  // Same chain, different money: this one is denominated in the SEP-24
  // reference anchor's USDC rather than a token the operator issues to itself,
  // so its reserves hold value that crossed a real fiat rail. Both are live and
  // neither replaces the other -- `testnet` is what the app, the docs and the
  // published SDK have always pointed at.
  "testnet-anchor": testnetAnchor as Deployment,
};

/** Whether an arbitrary value names a deployment this package carries. */
export function isNetworkName(value: unknown): value is NetworkName {
  return typeof value === "string" && (NETWORK_NAMES as readonly string[]).includes(value);
}

/**
 * Resolve a network selector that came from outside the type system — an
 * environment variable, a CLI flag, a query string.
 *
 * Pure: it reads no environment of its own, so it stays safe in the browser
 * bundle and is provable without one. `undefined` and an empty string both mean
 * "unspecified" and take the default; anything else must name a real
 * deployment, and the error says which names exist rather than only that this
 * one does not.
 */
export function parseNetwork(raw: string | undefined | null): NetworkName {
  const value = raw?.trim();
  if (!value) return DEFAULT_NETWORK;
  if (isNetworkName(value)) return value;
  throw new Error(
    `unknown network ${JSON.stringify(value)} — known: ${listNetworks().join(", ")}. ` +
      `Add deployments/<network>.json, a NETWORK_NAMES entry and a DEPLOYMENTS entry before using it.`,
  );
}

/**
 * Look up a committed deployment. `network` is optional and defaults to
 * `DEFAULT_NETWORK`.
 */
export function getDeployment(network: NetworkName = DEFAULT_NETWORK): Deployment {
  const deployment = DEPLOYMENTS[network];
  if (!deployment) {
    throw new Error(`unknown network: ${network}`);
  }
  return deployment;
}

export function listNetworks(): NetworkName[] {
  return [...NETWORK_NAMES];
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
    usdcIssuer: d.usdcIssuer,
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
