import type { NextConfig } from "next";

// Addresses and network endpoints are committed in deployments/*.json and
// imported by app code directly. No build-time env inlining — an address
// change is a code change, not a rebuild against a different environment.
//
// Secrets (*_SECRET, ANTHROPIC_API_KEY) stay in the real environment
// (.env.testnet locally, Vercel project env in production). Next auto-loads
// .env / .env.local; for local secrets we still accept .env.testnet via the
// accounts.ts dotenv bootstrap and the scripts' own loaders.
const nextConfig: NextConfig = {};

export default nextConfig;
