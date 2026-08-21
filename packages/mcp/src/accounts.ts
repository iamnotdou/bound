// Demo keypairs, loaded from the environment. SERVER ONLY — these hold secret
// keys, and every write tool in this package signs with one of them.
//
// This is a deliberate near-copy of apps/dashboard/app/lib/accounts.ts. The two
// read the SAME five environment variables, so a repo checkout keeps working
// either way, but a published package cannot reach into a Next.js app for its
// credentials — that import is exactly what stopped `mcp/server.ts` from being
// installable. Duplicating thirty lines is the cost of the package standing on
// its own; the env var names are the contract that keeps them in step.
//
// Bound is testnet-only (see the SDK's committed deployment record), so naming
// five fixed roles here is honest rather than limiting: there is no mainnet key
// anyone could lose. An MCP client supplies them through the `env` block of its
// server config; `.env.testnet` is only the in-repo convenience.
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
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

// An MCP client launches this server with cwd set to wherever it pleases, so we
// walk upward rather than assuming a fixed relative path. Skipped entirely once
// the environment already carries the keys, which is the normal case for an
// installed server: the client's config block is the supply route.
//
// dotenv is loaded through createRequire and swallowed on failure because it is
// NOT a dependency of this package — it is a dev-time convenience that happens
// to be present inside the Bound repo. An installed copy has no .env.testnet to
// find and never reaches this line.
if (!process.env.OPERATOR_SECRET) {
  const envPath = findEnvTestnet(process.cwd());
  if (envPath) {
    try {
      const load = createRequire(import.meta.url)("dotenv") as {
        config: (options: { path: string }) => void;
      };
      load.config({ path: envPath });
    } catch {
      // No dotenv on the resolution path: fall through and let kp() report the
      // missing variable by name, which is the more useful error anyway.
    }
  }
}

function kp(secretKey: string): Keypair {
  const secret = process.env[secretKey];
  if (!secret) {
    throw new Error(
      `missing ${secretKey} — set it in the MCP server's env block (see @bound/mcp README)`,
    );
  }
  return Keypair.fromSecret(secret);
}

// Lazily constructed so importing this module doesn't throw when only some
// roles are needed. A read-only session never touches a secret at all.
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
