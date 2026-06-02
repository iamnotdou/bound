// Backend route smoke test — proves the HTTP boundary a UI will connect to,
// without spinning up Next.js. Route handlers are just (Request) => Response
// functions, so we call them directly.
//
//   npx ts-node --transpile-only --project scripts/tsconfig.json scripts/routes-smoke.ts
//
// Covers:
//   • /api/paid-service  — the x402 server the agent's fetch_paid_service hits
//                          (402 challenge with price, then 200 on X-Payment)
//   • /api/verify        — cert lookup the dashboard reads (live testnet)
//   • /api/auditor (GET) — pending cert params + current cert state (live)
//   • /api/auditor (POST)— sign & publish; MUTATES state, only presence-checked
//   • app/lib/agent      — the AI-SDK agent module loads + wires the tools
//                          (live Claude streaming additionally needs an API key)
import { config as loadEnv } from "dotenv";
loadEnv({ path: ".env.testnet" });

import { GET as paidService } from "../app/api/paid-service/route";
import { POST as verifyPost } from "../app/api/verify/route";
import { GET as auditorGet, POST as auditorPost } from "../app/api/auditor/route";
import { GET as ledgerGet } from "../app/api/ledger/route";
import { POST as cheatPost } from "../app/api/cheat/route";
import { POST as operatorPost } from "../app/api/operator/route";
import { POST as challengerPost } from "../app/api/challenger/route";
import { runBoundAgent } from "../app/lib/agent";

const AGENT = process.env.AGENT_ADDRESS!;

function assert(cond: unknown, msg: string) {
  if (!cond) throw new Error(`ASSERT FAILED: ${msg}`);
}

async function main() {
  console.log("================ /api/paid-service (x402) ================");

  // 1 — no payment → 402 with the demanded price
  const r402 = await paidService(new Request("http://local/api/paid-service?price=250"));
  const b402 = await r402.json();
  console.log(`  no-payment  → ${r402.status}`, JSON.stringify(b402));
  assert(r402.status === 402, "expected 402 without payment");
  assert(b402.amount === 250, "expected demanded amount 250");
  assert(typeof b402.recipient === "string" && b402.recipient.startsWith("G"), "expected G... recipient");
  assert(b402.asset === "USDC", "expected USDC asset");

  // 2 — with an X-Payment proof → 200 with the content
  const r200 = await paidService(
    new Request("http://local/api/paid-service?price=250", {
      headers: { "X-Payment": "TESTTXHASH123" },
    }),
  );
  const b200 = await r200.json();
  console.log(`  with-payment→ ${r200.status}`, JSON.stringify(b200).slice(0, 120) + "…");
  assert(r200.status === 200, "expected 200 with payment");
  assert(b200.pricePaidUsd === 250, "expected pricePaidUsd 250");
  assert(b200.paymentRef === "TESTTXHASH123", "expected payment ref echoed");
  assert(typeof b200.data === "string" && b200.data.length > 0, "expected service data");

  console.log("  ✓ x402 server boundary works (402 → pay → 200)\n");

  console.log("================ /api/verify (cert lookup) ================");

  // bad address → 400
  const rBad = await verifyPost(new Request("http://local/api/verify", {
    method: "POST",
    body: JSON.stringify({ agent: "not-an-address" }),
  }));
  console.log(`  bad address → ${rBad.status}`);
  assert(rBad.status === 400, "expected 400 for malformed address");

  // real agent address → 200 with a CertView read from chain
  const rCert = await verifyPost(new Request("http://local/api/verify", {
    method: "POST",
    body: JSON.stringify({ agent: AGENT }),
  }));
  const cert = await rCert.json();
  console.log(`  agent cert  → ${rCert.status}`,
    JSON.stringify({ status: cert.status, valid: cert.valid, bound: cert.boundUsd, reserve: cert.reserveUsd }));
  assert(rCert.status === 200, "expected 200 for valid agent");
  assert(typeof cert.status === "string" && typeof cert.boundUsd === "string", "expected a CertView shape");
  console.log("  ✓ verify endpoint reads a real testnet certificate\n");

  console.log("================ /api/auditor (GET) ================");
  const rAud = await auditorGet();
  const aud = await rAud.json();
  console.log(`  pending     → ${rAud.status}`,
    JSON.stringify({ bound: aud.pending?.boundUsd, reserveClaimed: aud.pending?.reserveClaimedUsd, stake: aud.pending?.auditorStakeUsd }));
  assert(rAud.status === 200, "expected 200 from auditor GET");
  assert(aud.pending?.operator?.startsWith("G"), "expected operator address in pending");
  assert(typeof aud.current?.status === "string", "expected current cert view");
  assert(typeof auditorPost === "function", "auditor POST (sign-publish) must be exported");
  console.log("  ✓ auditor GET serves pending params + current cert (POST present, not invoked — it mutates state)\n");

  console.log("================ /api/ledger (proof board) ================");
  const rLedger = await ledgerGet();
  const ledger = await rLedger.json();
  console.log(`  ledger      → ${rLedger.status}`,
    JSON.stringify({
      accounts: ledger.accounts?.length,
      reserveHeld: ledger.contracts?.reserveHeldUsd,
      claimed: ledger.contracts?.reserveClaimedUsd,
      stake: ledger.contracts?.auditorStakeUsd,
      cert: ledger.cert?.status,
    }));
  assert(rLedger.status === 200, "expected 200 from ledger GET");
  assert(Array.isArray(ledger.accounts) && ledger.accounts.length === 5, "expected 5 actor balances");
  assert(ledger.accounts.every((a: any) => typeof a.usdc === "string" && a.address?.startsWith("G")), "expected role/usdc/address per actor");
  assert(typeof ledger.contracts?.reserveHeldUsd === "string", "expected reserveHeldUsd");
  assert(typeof ledger.cert?.status === "string", "expected cert view in ledger");
  console.log("  ✓ ledger serves all 5 balances + reserve held/claimed + stake + cert\n");

  console.log("================ /api/cheat (locks — read-only simulation) ================");
  // NOTE: the reserve/stake locks only bind while a cert is VERIFIED + unexpired.
  // When the live cert is Invalid (post-slash), nothing is locked and these
  // simulations legitimately succeed. So here we assert the MECHANISM (route
  // returns 200 + a boolean verdict) and report the live lock status. The
  // "lock holds" assertion belongs in the post-reset demo, where a fresh cert
  // is Verified — see FRONTEND.md §11.5.
  const certStatus = ledger.cert?.status;
  for (const action of ["withdraw-reserve", "withdraw-stake"] as const) {
    const r = await cheatPost(new Request("http://local/api/cheat", {
      method: "POST",
      body: JSON.stringify({ action }),
    }));
    const b = await r.json();
    console.log(`  ${action.padEnd(16)} → reverted=${b.reverted} (${b.expected ?? b.reason ?? "no active lock"})`);
    assert(r.status === 200, `expected 200 from cheat ${action}`);
    assert(typeof b.reverted === "boolean", `expected a boolean verdict from cheat ${action}`);
    if (certStatus === "Verified") {
      assert(b.reverted === true, `cert is Verified → ${action} MUST revert (the lock must hold)`);
    }
  }
  console.log(
    certStatus === "Verified"
      ? "  ✓ cert is Verified → defections trap on-chain (locks hold, no funds moved)\n"
      : `  ✓ cheat route works; cert is ${certStatus} so no locks are active — reset to Verified to prove the cage holds\n`,
  );

  console.log("================ /control write routes (presence only) ================");
  // These MUTATE state / spend testnet funds, so we only assert they are wired.
  assert(typeof operatorPost === "function", "operator POST (deposit/publish) must be exported");
  assert(typeof challengerPost === "function", "challenger POST (challenge+resolve) must be exported");
  console.log("  ✓ operator + challenger POST present (not invoked — they mutate state)\n");

  console.log("================ app/lib/agent (AI SDK) ================");
  const stream = runBoundAgent([{ role: "user", content: "ping" }]);
  assert(typeof (stream as any)?.toDataStreamResponse === "function", "runBoundAgent must return a streamText result");
  console.log("  ✓ agent module loads and returns a streamable result");
  const hasKey = !!process.env.ANTHROPIC_API_KEY;
  console.log(
    hasKey
      ? "  ✓ ANTHROPIC_API_KEY present — /api/chat will stream live Claude responses"
      : "  ⚠ ANTHROPIC_API_KEY EMPTY — tool layer works, but /api/chat won't stream until a key is set",
  );

  console.log("\n✓ Route/HTTP boundary is ready for a UI to connect.");
}

main().catch((e) => {
  console.error("✗ routes-smoke failed:", e);
  process.exit(1);
});
