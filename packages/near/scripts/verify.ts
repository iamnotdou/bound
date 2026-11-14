// pnpm near:verify <agent.testnet> [--json] -- zero-secret: what a
// counterparty reads before dealing with an agent. Exit 0 if Verified,
// 2 if the agent has a certificate in any other state, 3 if none.

import { accountCodeOf, testnetProvider, view, withRetry } from "../src/client.js";
import { loadDeployment } from "../tooling/deployment.js";
import {
  assess,
  coverageOf,
  describeCert,
  describeCode,
  fmtUsdc,
  isConfined,
  type AccessKeyInfo,
  type CertViewJson,
} from "../src/format.js";

async function main() {
  const agent = process.argv.slice(2).find((a) => !a.startsWith("--"));
  const d = loadDeployment();
  const target = agent ?? d.demoRuns.at(-1)?.agent2;
  if (!target) throw new Error("usage: pnpm near:verify <agent account id>");
  const provider = testnetProvider();
  const v = await view<CertViewJson | null>(provider, d.contracts.bound.accountId, "verify", {
    agent: target,
  });
  if (process.argv.includes("--json")) console.log(JSON.stringify(v, null, 2));
  if (!v) {
    console.log(`${target}: no Bound certificate. Do not extend credit.`);
    process.exit(3);
  }
  const a = assess(v);
  console.log(`${target}: ${describeCert(v)}`);
  console.log(
    `  coverage ${fmtUsdc(coverageOf(v))} = reserve ${fmtUsdc(v.reserve_locked)} + auditor allocation ${fmtUsdc(v.allocation)} (the most arbitrated victims recover, shared by every creditor)`,
  );
  // verify() cannot see the agent's keys or code: read both here.
  const keys = (
    (await withRetry(() =>
      provider.viewAccessKeyList({
        accountId: target,
        finalityQuery: { finality: "final" },
      } as never),
    )) as unknown as { keys: AccessKeyInfo[] }
  ).keys;
  const code = await accountCodeOf(provider, target);
  const confined = isConfined(keys, d.contracts.bound.accountId, code);
  console.log(
    `  confinement: ${confined ? "confined" : "NOT confined"} (${keys.length} key(s), ${describeCode(code)})`,
  );
  if (v.settlement) console.log(`  settled: ${JSON.stringify(v.settlement)}`);
  console.log(`  decision: ${a.ok ? "deal (Verified)" : `refuse (${a.reason})`}`);
  process.exit(a.ok ? 0 : 2);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
