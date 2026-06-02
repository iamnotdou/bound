// LIVE full-loop dry-run — proves the whole demo on testnet end to end.
// Spends a small amount of testnet USDC and re-seeds the cert (the state you
// want before demoing anyway). Drives the same route handlers + client the UI
// will call, in the run-of-show order.
//
//   pnpm dlx ts-node --transpile-only --project scripts/tsconfig.json scripts/dry-run.ts
import { config as loadEnv } from "dotenv";
loadEnv({ path: ".env.testnet" });

import { POST as auditorPost } from "../app/api/auditor/route";
import { POST as operatorPost } from "../app/api/operator/route";
import { POST as challengerPost } from "../app/api/challenger/route";
import { POST as cheatPost } from "../app/api/cheat/route";
import { GET as ledgerGet } from "../app/api/ledger/route";
import { bound } from "../app/lib/bound-client";
import { accounts } from "../app/lib/accounts";
import { usdc, formatUsdc } from "../app/lib/config";

const j = (r: Response) => r.json();
const post = (h: (req: Request) => Promise<Response>, body: unknown) =>
  h(new Request("http://local", { method: "POST", body: JSON.stringify(body) })).then(j);

async function ledger(label: string) {
  const l = await ledgerGet().then(j);
  const row = (r: any) => `${r.role.padEnd(13)} ${r.usdc}`;
  console.log(`\n── ledger: ${label} ──`);
  l.accounts.forEach((a: any) => console.log("   " + row(a)));
  console.log(`   reserve held ${l.contracts.reserveHeldUsd} / claimed ${l.contracts.reserveClaimedUsd} · stake ${l.contracts.auditorStakeUsd} · cert ${l.cert.status}`);
  return l;
}

async function main() {
  console.log("======== BOUND PROTOCOL — LIVE FULL-LOOP DRY-RUN ========");
  await ledger("start (expected: cert Invalid, reserve $0)");

  // 1 — Seed: operator publishes + auditor attests → VERIFIED
  console.log("\n[1/6] Seed cert (auditor sign & publish)…");
  const seed = await post(auditorPost, { action: "sign-publish" });
  if (seed.error) throw new Error("seed failed: " + seed.error);
  console.log(`   ✓ cert #${seed.certId} → ${seed.cert.status} (bound ${seed.cert.boundUsd}, claims ${seed.cert.reserveUsd})`);
  const certId = seed.certId;

  // 2 — Cheat lane: while the cert is VERIFIED both locks must hold.
  //   • STAKE lock — set per-cert by attest() → auditor can't walk staked capital.
  //   • RESERVE lock — global unlock_at (now +365d on this fresh deploy) → operator
  //     can't reclaim the reserve early.
  console.log("\n[2/6] Adversarial defections must REVERT while cert is Verified…");
  for (const action of ["withdraw-stake", "withdraw-reserve"] as const) {
    const c = await post(cheatPost, { action });
    const ok = c.reverted === true;
    console.log(`   ${ok ? "✓" : "✗"} ${action.padEnd(16)} reverted=${c.reverted} ${c.expected ? `(${c.expected})` : `(${c.reason ?? "NO REVERT — lock failed"})`}`);
    if (!ok) throw new Error(`${action} did NOT revert while cert Verified — lock not enforced`);
  }

  // 3 — Operator under-funds the reserve ($4k vs $10k claimed): set the trap
  console.log("\n[3/6] Operator deposits $4,000 into the reserve (claims $10,000)…");
  const dep = await post(operatorPost, { action: "deposit-reserve", amountUsd: 4000 });
  if (dep.error) throw new Error("deposit failed: " + dep.error);
  console.log(`   ✓ reserve held ${dep.reserveHeldUsd} / claimed ${dep.claimedUsd}`);

  // 4 — Agent autonomously pays the counterparty $500
  console.log("\n[4/6] Agent pays $500 USDC to the counterparty…");
  const hash = await bound.executePayment(accounts.agent, accounts.counterparty.publicKey(), usdc(500));
  console.log(`   ✓ tx ${hash}`);

  await ledger("after pay + under-funded reserve");

  // 5 — Challenger proves the short reserve → SLASH
  console.log("\n[5/6] Challenger proves InsufficientReserve → resolve()…");
  const chal = await post(challengerPost, { certId });
  if (chal.error) throw new Error("challenge failed: " + chal.error);
  console.log(`   ${chal.outcome}`);
  console.log(`   victim ${chal.victimUsdBefore} → ${chal.victimUsdAfter} · auditor stake → ${chal.auditorStakeAfterUsd} · reserve → ${chal.reserveAfterUsd}`);
  console.log(`   cert ${chal.certBefore.status} → ${chal.certAfter.status}`);
  if (chal.certAfter.status !== "Invalid") throw new Error("challenge did not invalidate the cert");

  // 6 — Final board
  await ledger("end (expected: cert Invalid, stake $0, victim up)");

  console.log("\n======== ✓ FULL LOOP PASSED — seed → locks hold → pay → SLASH ========");
  console.log("Note: cert is now Invalid again (the resting state). Re-run [1] /auditor to reset before the live demo.");
}

main().catch((e) => {
  console.error("\n✗ dry-run failed:", e.message ?? e);
  process.exit(1);
});
