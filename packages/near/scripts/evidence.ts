// pnpm near:evidence [file.json ...] -- zero-secret: re-checks every recorded
// transaction hash against RPC `tx` status (archival node) and prints OK/FAIL
// per step, so a reviewer that cannot open NearBlocks can still prove the
// runs happened. Default: every demo run and MCP agent run under
// near/evidence/ (and near/evidence/bound-v1/, the superseded contract), plus
// the deployment transactions in near/deployments/testnet.json.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { TESTNET_ARCHIVAL_RPCS, testnetProvider, withRetry } from "../src/client.js";
import { EVIDENCE_DIR, REPO_ROOT, loadDeployment } from "../tooling/deployment.js";

interface Check {
  label: string;
  hash: string | null;
  signer: string;
  ok: boolean;
  note?: string;
}

const hashOf = (url: unknown) => (typeof url === "string" ? (url.split("/").pop() ?? null) : null);

function fromFile(f: string): { title: string; checks: Check[]; pass: boolean } {
  const ev = JSON.parse(readFileSync(f, "utf8")) as Record<string, unknown>;
  if (Array.isArray(ev.steps) && /mcp-agent-/.test(f)) {
    // MCP agent run: steps carry detail.tx URLs; "refused" steps are
    // expected to be failed transactions.
    const agent = String(ev.agent);
    const checks = (ev.steps as Array<{ label: string; detail?: { tx?: string } }>)
      .filter((s) => s.detail?.tx)
      .map((s) => ({
        label: s.label,
        hash: hashOf(s.detail!.tx),
        signer: agent,
        ok: !/refused/.test(s.label),
      }));
    return { title: `MCP agent run ${ev.runId} (${agent})`, checks, pass: ev.pass === true };
  }
  const steps = ev.steps as Array<{
    act: number;
    label: string;
    signer: string;
    hash: string | null;
    ok: boolean;
    error: string | null;
  }>;
  return {
    title: `demo run ${ev.runId} on ${ev.contract}`,
    checks: steps.map((s) => ({
      label: `act ${s.act} ${s.label}`,
      hash: s.hash,
      signer: s.signer,
      ok: s.ok,
      note: s.hash ? undefined : (s.error ?? "").slice(0, 80),
    })),
    pass: ev.pass === true,
  };
}

function defaultFiles(): string[] {
  const dirs = [EVIDENCE_DIR, join(EVIDENCE_DIR, "bound-v1")].filter((d) => existsSync(d));
  return dirs.flatMap((d) =>
    readdirSync(d)
      .filter((f) => /^(near-demo|mcp-agent)-.*\.json$/.test(f))
      .map((f) => join(d, f)),
  );
}

async function main() {
  const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
  const groups = (args.length ? args.map((a) => resolve(REPO_ROOT, a)) : defaultFiles()).map(
    (f) => ({ file: f, ...fromFile(f) }),
  );
  if (!args.length) {
    const d = loadDeployment();
    const dep = (title: string, txs: Array<{ step: string; hash: string }>) => ({
      file: "near/deployments/testnet.json",
      title,
      checks: txs.map((t) => ({ label: t.step, hash: t.hash, signer: d.deployer, ok: true })),
      pass: true,
    });
    groups.push(dep(`deployment of ${d.contracts.bound.accountId}`, d.txs));
    for (const p of d.previous ?? []) groups.push(dep(`superseded ${p.accountId}`, p.txs));
  }
  if (!groups.length) throw new Error(`no evidence files in ${EVIDENCE_DIR}`);
  const p = testnetProvider(TESTNET_ARCHIVAL_RPCS);
  let bad = 0;
  let n = 0;
  for (const g of groups) {
    console.log(`${g.file}: ${g.title}, recorded pass=${g.pass}`);
    for (const s of g.checks) {
      if (!s.hash) {
        console.log(`  --   ${s.label}: no transaction by design (${s.note ?? ""})`);
        continue;
      }
      // Paced: the free archival tier answers -429 to bursts of ~40 lookups.
      await new Promise((r) => setTimeout(r, 300));
      try {
        const r = (await withRetry(
          () =>
            p.viewTransactionStatus({
              txHash: s.hash!,
              accountId: s.signer,
              waitUntil: "FINAL",
            } as never),
          5,
        )) as unknown as { status: Record<string, unknown> };
        const ok = "SuccessValue" in r.status;
        const match = ok === s.ok;
        if (!match) bad++;
        n++;
        console.log(
          `  ${match ? "OK  " : "FAIL"} ${s.hash} ${ok ? "succeeded" : "failed"} on chain, recorded ${s.ok ? "succeeded" : "failed"}: ${s.label}`,
        );
      } catch (e) {
        bad++;
        console.log(
          `  FAIL ${s.hash}: not found (${e instanceof Error ? e.message.slice(0, 120) : e})`,
        );
      }
    }
    if (!g.pass) bad++;
  }
  console.log(bad ? `${bad} problem(s)` : `all ${n} recorded transactions match the chain`);
  process.exit(bad ? 1 : 0);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
