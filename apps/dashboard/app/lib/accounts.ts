// Demo keypairs, loaded server-side from the environment. SERVER ONLY — these
// hold secret keys and must never be bundled into the browser.
//
// The secrets live in .env.testnet at the repo root. Next.js runs with cwd
// set to apps/dashboard/, and root-run scripts/mcp run with cwd at the repo
// root, so we walk upward from process.cwd() to find it rather than assuming
// a fixed relative path.
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { Keypair } from "@stellar/stellar-sdk";

function findEnvTestnet(startDir: string): string | undefined {
  let dir = startDir;
  for (;;) {
    const candidate = join(dir, ".env.testnet");
    if (existsSync(candidate)) return candidate;
    const parent = dirname(dir);
    if (parent === dir) return undefined;
    dir = parent;
  }
}

if (typeof window === "undefined" && !process.env.OPERATOR_SECRET) {
  const envPath = findEnvTestnet(process.cwd());
  if (envPath) {
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    require("dotenv").config({ path: envPath });
  }
}

function kp(secretKey: string): Keypair {
  const secret = process.env[secretKey];
  if (!secret) throw new Error(`missing ${secretKey} — run \`pnpm setup\``);
  return Keypair.fromSecret(secret);
}

// Lazily constructed so importing this module doesn't throw when only some
// roles are needed.
export const accounts = {
  get operator() {
    return kp("OPERATOR_SECRET");
  },
  get agent() {
    return kp("AGENT_SECRET");
  },
  get auditor() {
    return kp("AUDITOR_SECRET");
  },
  get challenger() {
    return kp("CHALLENGER_SECRET");
  },
  get counterparty() {
    return kp("COUNTERPARTY_SECRET");
  },
};
