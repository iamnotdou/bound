// pnpm near:evidence [near/evidence/near-demo-<runid>.json] -- zero-secret:
// re-checks every transaction hash in committed demo evidence against RPC
// `tx` status and prints OK/FAIL per step, so a reviewer that cannot open
// NearBlocks can still prove the run happened. Default: every evidence file.

import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { TESTNET_ARCHIVAL_RPCS, testnetProvider, withRetry } from "../src/client.js";
import { EVIDENCE_DIR, REPO_ROOT } from "../src/deployment.js";

interface Step {
  act: number;
  label: string;
  signer: string;
  hash: string | null;
  ok: boolean;
  error: string | null;
}

async function main() {
  const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
  const files = args.length
    ? args.map((a) => resolve(REPO_ROOT, a))
    : readdirSync(EVIDENCE_DIR)
        .filter((f) => /^near-demo-.*\.json$/.test(f))
        .map((f) => join(EVIDENCE_DIR, f));
  if (!files.length) throw new Error(`no evidence files in ${EVIDENCE_DIR}`);
  const p = testnetProvider(TESTNET_ARCHIVAL_RPCS);
  let bad = 0;
  for (const f of files) {
    const ev = JSON.parse(readFileSync(f, "utf8")) as {
      runId: string;
      steps: Step[];
      pass: boolean;
    };
    console.log(`${f}: run ${ev.runId}, recorded pass=${ev.pass}`);
    for (const s of ev.steps) {
      if (!s.hash) {
        console.log(
          `  --   act ${s.act} ${s.label}: no transaction by design (${(s.error ?? "").slice(0, 80)})`,
        );
        continue;
      }
      // Paced: the free archival tier answers -429 to bursts of ~40 lookups.
      await new Promise((r) => setTimeout(r, 300));
      try {
        const r = (await withRetry(() =>
          p.viewTransactionStatus({
            txHash: s.hash!,
            accountId: s.signer,
            waitUntil: "FINAL",
          } as never),
        )) as unknown as { status: Record<string, unknown> };
        const ok = "SuccessValue" in r.status;
        const match = ok === s.ok;
        if (!match) bad++;
        console.log(
          `  ${match ? "OK  " : "FAIL"} act ${s.act} ${s.hash} ${ok ? "succeeded" : "failed"} on chain, recorded ${s.ok ? "succeeded" : "failed"}: ${s.label}`,
        );
      } catch (e) {
        bad++;
        console.log(
          `  FAIL act ${s.act} ${s.hash}: not found (${e instanceof Error ? e.message.slice(0, 120) : e})`,
        );
      }
    }
    if (!ev.pass) bad++;
  }
  console.log(bad ? `${bad} problem(s)` : "all recorded transactions match the chain");
  process.exit(bad ? 1 : 0);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
