// pnpm near:verify <agent.testnet> [--json] -- zero-secret: what a
// counterparty reads before dealing with an agent. Exit 0 if Verified,
// 2 if the agent has a certificate in any other state, 3 if none.

import { testnetProvider, view } from "../src/client.js";
import { loadDeployment } from "../src/deployment.js";
import { assess, describeCert, fmtUsdc, type CertViewJson } from "../src/format.js";

async function main() {
  const agent = process.argv.slice(2).find((a) => !a.startsWith("--"));
  const d = loadDeployment();
  const target = agent ?? d.demoRuns.at(-1)?.agent2;
  if (!target) throw new Error("usage: pnpm near:verify <agent account id>");
  const v = await view<CertViewJson | null>(
    testnetProvider(),
    d.contracts.bound.accountId,
    "verify",
    { agent: target },
  );
  if (process.argv.includes("--json")) console.log(JSON.stringify(v, null, 2));
  if (!v) {
    console.log(`${target}: no Bound certificate. Do not extend credit.`);
    process.exit(3);
  }
  const a = assess(v);
  console.log(`${target}: ${describeCert(v)}`);
  console.log(
    `  worst case covered: reserve ${fmtUsdc(v.reserve_locked)} + auditor allocation ${fmtUsdc(v.allocation)}`,
  );
  if (v.settlement) console.log(`  settled: ${JSON.stringify(v.settlement)}`);
  console.log(`  decision: ${a.ok ? "deal (Verified)" : `refuse (${a.reason})`}`);
  process.exit(a.ok ? 0 : 2);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
