// Server-side configuration for the Bound SDK.
//
// Public configuration (network endpoints, contract addresses, the RPC
// read-source account) comes from the committed deployments map. Credentials
// (*_SECRET, ANTHROPIC_API_KEY) stay in the environment and never enter this
// package — see `@bound/mcp`'s accounts.ts, or the consuming app's own.
//
// Never import this into browser code: it is the server half of the split.
// Client code takes `@bound/sdk/deployments` instead.
import { getDeployment, parseNetwork } from "./deployments";

/**
 * Which deployment this process talks to.
 *
 * `STELLAR_NETWORK` is validated against the deployments the package actually
 * carries rather than against a literal written here, so a second deployment is
 * added in `deployments.ts` alone and every consumer of this module follows
 * without an edit. Unset means the default.
 */
export const networkName = parseNetwork(process.env.STELLAR_NETWORK);

const deployment = getDeployment(networkName);

export const network = {
  rpcUrl: deployment.rpcUrl,
  horizonUrl: deployment.horizonUrl,
  passphrase: deployment.networkPassphrase,
};

// Read-only simulations still need a real, funded source account (the RPC reads
// its sequence number). Public G... key, committed in the deployments file.
export const readSource = deployment.readSource;

// Demo actor public keys (G...). Secrets for these live only in env (accounts.ts).
export const accounts = deployment.accounts;

export const contracts = {
  registry: deployment.contracts.registry,
  reserveVault: deployment.contracts.reserveVault,
  auditorStaking: deployment.contracts.auditorStaking,
  feeEscrow: deployment.contracts.feeEscrow,
  challengeManager: deployment.contracts.challengeManager,
  // undefined on a v1 deployment record — see Deployment.contracts.paymentRouter.
  paymentRouter: deployment.contracts.paymentRouter,
  premiumVault: deployment.contracts.premiumVault,
  usdc: deployment.contracts.usdc,
};

// USDC on Stellar uses 7 decimals.
export const USDC_DECIMALS = 7n;

/** Convert dollars → USDC stroops (bigint, what the contracts expect). */
export function usdc(dollars: number): bigint {
  return BigInt(Math.round(dollars * 100)) * 10n ** (USDC_DECIMALS - 2n);
}

/** Convert USDC stroops → a human dollar string. */
export function formatUsdc(stroops: bigint): string {
  return `$${(Number(stroops) / 1e7).toLocaleString()}`;
}
