// Server-side configuration for the Bound SDK. Reads the deployed contract
// addresses and network settings from the environment. Never import this into
// browser code — it pulls in secrets and Node-only modules.
//
// Outside Next.js (scripts, smoke tests) we lazily load .env.testnet so the SDK
// works the same way the deploy/demo scripts do.
if (typeof window === "undefined" && !process.env.REGISTRY_ADDRESS) {
  // eslint-disable-next-line @typescript-eslint/no-var-requires
  require("dotenv").config({ path: ".env.testnet" });
}

function req(key: string): string {
  const v = process.env[key];
  if (!v) throw new Error(`missing ${key} — run \`pnpm setup\` && \`pnpm deploy\``);
  return v;
}

export const network = {
  rpcUrl: process.env.STELLAR_RPC_URL ?? "https://soroban-testnet.stellar.org",
  passphrase: process.env.STELLAR_NETWORK_PASSPHRASE ?? "Test SDF Network ; September 2015",
};

// Read-only simulations still need a real, funded source account (the RPC reads
// its sequence number). The operator works fine for that.
export const readSource = req("OPERATOR_ADDRESS");

export const contracts = {
  registry: req("REGISTRY_ADDRESS"),
  reserveVault: req("RESERVE_VAULT_ADDRESS"),
  auditorStaking: req("AUDITOR_STAKING_ADDRESS"),
  feeEscrow: req("FEE_ESCROW_ADDRESS"),
  challengeManager: req("CHALLENGE_MANAGER_ADDRESS"),
  usdc: req("USDC_ADDRESS"),
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
