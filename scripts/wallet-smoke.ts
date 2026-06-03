// Smoke the connected-wallet tx-build path against live testnet WITHOUT signing
// or submitting. Confirms the bindings produce a valid, simulated, unsigned XDR
// envelope when given a wallet public key and no signer — the exact shape the
// browser wallet will sign. Pure reads/simulation; no funds move.
//
//   pnpm wallet-smoke
import { buildActionXdr, buildTrustlineXdr } from "../app/lib/tx-build";

const AGENT = process.env.AGENT_ADDRESS!;
const COUNTERPARTY = process.env.COUNTERPARTY_ADDRESS!;

function ok(label: string, xdr: string) {
  const looksValid = typeof xdr === "string" && xdr.length > 50;
  console.log(`  ${looksValid ? "✓" : "✗"} ${label} → ${xdr.slice(0, 24)}… (${xdr.length} chars)`);
  if (!looksValid) throw new Error(`${label} produced no XDR`);
}

async function main() {
  console.log("================ /api/tx build (unsigned, simulated) ================");

  // Soroban contract call: agent pays $1 USDC → counterparty. Simulates cleanly
  // (agent is funded). Built with publicKey=AGENT, no signer.
  ok("pay $1 (agent→counterparty)", await buildActionXdr("pay", AGENT, { to: COUNTERPARTY, amountUsd: 1 }));

  // Classic changeTrust to USDC:OPERATOR — the wallet funding trustline step.
  ok("trustline (changeTrust USDC)", await buildTrustlineXdr(AGENT));

  console.log("\n✓ Wallet tx-build path is ready — unsigned XDR builds against live testnet.");
}

main().catch((e) => {
  console.error("✗ wallet-smoke failed:", e.message);
  process.exit(1);
});
