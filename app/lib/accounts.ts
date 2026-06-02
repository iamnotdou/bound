// Demo keypairs, loaded server-side from the environment. SERVER ONLY — these
// hold secret keys and must never be bundled into the browser.
import { Keypair } from "@stellar/stellar-sdk";

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
