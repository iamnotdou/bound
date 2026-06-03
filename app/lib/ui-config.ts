// Client-safe configuration. Reads ONLY the public values exposed to the browser
// via next.config.ts `env` (addresses + network endpoints — never a secret key).
//
// This is deliberately separate from app/lib/config.ts, which is server-only
// (it pulls in dotenv + secret keys). Never import config.ts from client code.

export const NETWORK = process.env.STELLAR_NETWORK ?? "testnet";

export const network = {
  rpcUrl: process.env.STELLAR_RPC_URL ?? "https://soroban-testnet.stellar.org",
  horizonUrl: process.env.STELLAR_HORIZON_URL ?? "https://horizon-testnet.stellar.org",
  passphrase:
    process.env.STELLAR_NETWORK_PASSPHRASE ?? "Test SDF Network ; September 2015",
};

export type RoleKey = "operator" | "agent" | "auditor" | "counterparty" | "challenger";

export const roles: Record<RoleKey, { label: string; address: string }> = {
  operator: { label: "Operator", address: process.env.OPERATOR_ADDRESS ?? "" },
  agent: { label: "Agent", address: process.env.AGENT_ADDRESS ?? "" },
  auditor: { label: "Auditor", address: process.env.AUDITOR_ADDRESS ?? "" },
  counterparty: { label: "Counterparty", address: process.env.COUNTERPARTY_ADDRESS ?? "" },
  challenger: { label: "Challenger", address: process.env.CHALLENGER_ADDRESS ?? "" },
};

export const contracts: Record<string, { label: string; id: string }> = {
  registry: { label: "Registry", id: process.env.REGISTRY_ADDRESS ?? "" },
  reserveVault: { label: "ReserveVault", id: process.env.RESERVE_VAULT_ADDRESS ?? "" },
  auditorStaking: { label: "AuditorStaking", id: process.env.AUDITOR_STAKING_ADDRESS ?? "" },
  feeEscrow: { label: "FeeEscrow", id: process.env.FEE_ESCROW_ADDRESS ?? "" },
  challengeManager: { label: "ChallengeManager", id: process.env.CHALLENGE_MANAGER_ADDRESS ?? "" },
  usdc: { label: "USDC (SAC)", id: process.env.USDC_ADDRESS ?? "" },
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
