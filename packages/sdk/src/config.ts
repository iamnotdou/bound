// Server-side configuration for the Bound SDK.
//
// Public configuration (network endpoints, contract addresses, the RPC
// read-source account) comes from the committed deployments map. Credentials
// (*_SECRET, ANTHROPIC_API_KEY) stay in the environment and never enter this
// package — see apps/dashboard/app/lib/accounts.ts.
//
// Never import this into browser code: it is the server half of the split.
// Client code takes `@bound/sdk/deployments` instead.
import { getDeployment, type NetworkName } from "./deployments";

function resolveNetwork(): NetworkName {
  const raw = process.env.STELLAR_NETWORK;
  if (!raw || raw === "testnet") return "testnet";
  throw new Error(
    `unknown STELLAR_NETWORK=${JSON.stringify(raw)} — known: testnet. ` +
      `Add a deployments/<network>.json and a DEPLOYMENTS entry before using it.`,
  );
}

const deployment = getDeployment(resolveNetwork());

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
