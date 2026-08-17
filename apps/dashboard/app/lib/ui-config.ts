// Client-safe configuration. Reads committed public data from the deployments
// map — never a secret key, never process.env for addresses.
//
// This is deliberately separate from app/lib/config.ts only in spirit (money
// helpers live there); both now share getDeployment() as the address source.
// Safe to import from Client Components.
import { getDeployment, type NetworkName } from "./deployments";

const deployment = getDeployment(
  (process.env.STELLAR_NETWORK as NetworkName | undefined) ?? "testnet",
);

export const NETWORK: NetworkName = deployment.network;

export const network = {
  rpcUrl: deployment.rpcUrl,
  horizonUrl: deployment.horizonUrl,
  passphrase: deployment.networkPassphrase,
};

/**
 * True when this deployment targets Stellar mainnet. Derived from the network
 * passphrase rather than the network name, because the passphrase is what the
 * chain itself uses to distinguish them — and it stays a runtime check even
 * while NetworkName has only one member.
 */
export const IS_PUBLIC_NETWORK =
  deployment.networkPassphrase === "Public Global Stellar Network ; September 2015";

export type RoleKey = "operator" | "agent" | "auditor" | "counterparty" | "challenger";

export const roles: Record<RoleKey, { label: string; address: string }> = {
  operator: { label: "Operator", address: deployment.accounts.operator },
  agent: { label: "Agent", address: deployment.accounts.agent },
  auditor: { label: "Auditor", address: deployment.accounts.auditor },
  counterparty: { label: "Counterparty", address: deployment.accounts.counterparty },
  challenger: { label: "Challenger", address: deployment.accounts.challenger },
};

export const contracts: Record<string, { label: string; id: string }> = {
  registry: { label: "Registry", id: deployment.contracts.registry },
  reserveVault: { label: "ReserveVault", id: deployment.contracts.reserveVault },
  auditorStaking: { label: "AuditorStaking", id: deployment.contracts.auditorStaking },
  feeEscrow: { label: "FeeEscrow", id: deployment.contracts.feeEscrow },
  challengeManager: { label: "ChallengeManager", id: deployment.contracts.challengeManager },
  usdc: { label: "USDC (SAC)", id: deployment.contracts.usdc },
};

/** Truncate a Stellar address/contract id / tx hash for display: GABC…WXYZ */
export function truncate(value: string, head = 4, tail = 4): string {
  if (!value || value.length <= head + tail + 1) return value;
  return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

/** Map a known public address back to its role label, if any. */
export function roleForAddress(address: string): string | null {
  const hit = Object.values(roles).find((r) => r.address === address);
  return hit?.label ?? null;
}

/** Format a raw dollar number as a USD string (the API mostly returns these pre-formatted). */
export function formatUsd(dollars: number): string {
  return `$${dollars.toLocaleString(undefined, { maximumFractionDigits: 2 })}`;
}
