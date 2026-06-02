import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  env: {
    STELLAR_NETWORK: process.env.STELLAR_NETWORK,
    STELLAR_RPC_URL: process.env.STELLAR_RPC_URL,
    OPERATOR_ADDRESS: process.env.OPERATOR_ADDRESS,
    AGENT_ADDRESS: process.env.AGENT_ADDRESS,
    AUDITOR_ADDRESS: process.env.AUDITOR_ADDRESS,
    COUNTERPARTY_ADDRESS: process.env.COUNTERPARTY_ADDRESS,
    USDC_ADDRESS: process.env.USDC_ADDRESS,
  },
};

export default nextConfig;
