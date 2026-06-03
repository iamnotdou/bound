import type { NextConfig } from "next";
import { config as loadEnv } from "dotenv";

// Next auto-loads .env / .env.local but NOT .env.testnet, where this project
// keeps its config — so without this the `env` block below would resolve every
// address to undefined and the client would get empty role addresses. Load it
// here (dotenv won't override real env vars, and no-ops if the file is absent,
// so Vercel-set vars still win). Server code bootstraps the same file via
// app/lib/config.ts.
loadEnv({ path: ".env.testnet" });

// Public, non-secret values exposed to the browser. These are addresses and
// network endpoints only — never a secret key. The 5 actor SECRETs and the
// ANTHROPIC key stay server-side (read directly from process.env in routes).
const nextConfig: NextConfig = {
  env: {
    STELLAR_NETWORK: process.env.STELLAR_NETWORK,
    STELLAR_RPC_URL: process.env.STELLAR_RPC_URL,
    STELLAR_HORIZON_URL: process.env.STELLAR_HORIZON_URL,
    STELLAR_NETWORK_PASSPHRASE: process.env.STELLAR_NETWORK_PASSPHRASE,

    // actor public addresses (challenger included — it's the role a judge can
    // also play with their own wallet; the SECRET stays server-side)
    OPERATOR_ADDRESS: process.env.OPERATOR_ADDRESS,
    AGENT_ADDRESS: process.env.AGENT_ADDRESS,
    AUDITOR_ADDRESS: process.env.AUDITOR_ADDRESS,
    COUNTERPARTY_ADDRESS: process.env.COUNTERPARTY_ADDRESS,
    CHALLENGER_ADDRESS: process.env.CHALLENGER_ADDRESS,

    // deployed contract ids (public)
    USDC_ADDRESS: process.env.USDC_ADDRESS,
    REGISTRY_ADDRESS: process.env.REGISTRY_ADDRESS,
    RESERVE_VAULT_ADDRESS: process.env.RESERVE_VAULT_ADDRESS,
    AUDITOR_STAKING_ADDRESS: process.env.AUDITOR_STAKING_ADDRESS,
    FEE_ESCROW_ADDRESS: process.env.FEE_ESCROW_ADDRESS,
    CHALLENGE_MANAGER_ADDRESS: process.env.CHALLENGE_MANAGER_ADDRESS,
  },
};

export default nextConfig;
