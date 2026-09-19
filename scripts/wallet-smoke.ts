// Smoke the connected-wallet tx-build path against live testnet WITHOUT signing
// or submitting. Confirms the bindings produce a valid, simulated, unsigned XDR
// envelope when given a wallet public key and no signer — the exact shape the
// browser wallet will sign. Pure reads/simulation; no funds move.
//
//   pnpm wallet-smoke
import { buildActionXdr, buildTrustlineXdr } from "@bound/sdk";
import { readEnv } from "./lib";

// Read the env file explicitly. This used to arrive as a side effect of
// importing the dashboard's tx-build shim, which pulled in accounts.ts and its
// dotenv bootstrap. The shim was a pure re-export of @bound/sdk, so importing
// the SDK directly dropped the hidden bootstrap with it.
const env = readEnv();
const AGENT = env.AGENT_ADDRESS;
const COUNTERPARTY = env.COUNTERPARTY_ADDRESS;

function ok(label: string, xdr: string) {
  const looksValid = typeof xdr === "string" && xdr.length > 50;
  console.log(`  ${looksValid ? "✓" : "✗"} ${label} → ${xdr.slice(0, 24)}… (${xdr.length} chars)`);
  if (!looksValid) throw new Error(`${label} produced no XDR`);
}

async function main() {
  console.log("================ /api/tx build (unsigned, simulated) ================");

  // Soroban contract call: agent pays $1 USDC → counterparty. Simulates cleanly
  // (agent is funded). Built with publicKey=AGENT, no signer.
  ok(
    "pay $1 (agent→counterparty)",
    await buildActionXdr("pay", AGENT, { to: COUNTERPARTY, amountUsd: 1 }),
  );

  // Classic changeTrust to USDC:OPERATOR — the wallet funding trustline step.
  ok("trustline (changeTrust USDC)", await buildTrustlineXdr(AGENT));

  console.log("\n✓ Wallet tx-build path is ready — unsigned XDR builds against live testnet.");
}

main().catch((e) => {
  console.error("✗ wallet-smoke failed:", e.message);
  process.exit(1);
});
