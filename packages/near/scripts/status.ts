// pnpm near:status -- zero-secret: prints the deployment, its on-chain config,
// code_hash check and certificate count. Reads only; needs no credentials.

import { testnetProvider, view, withRetry } from "../src/client.js";
import { loadDeployment } from "../src/deployment.js";
import { explorerAccount, explorerTx } from "../src/format.js";

async function main() {
  const d = loadDeployment();
  const p = testnetProvider();
  const B = d.contracts.bound.accountId;
  console.log(`Bound on NEAR ${d.network}`);
  console.log(`  contract  ${B}  ${explorerAccount(B)}`);
  console.log(`  token     ${d.contracts.token.accountId} (${d.contracts.token.note})`);
  console.log(`  treasury  ${d.accounts.treasury}   arbiter ${d.accounts.arbiter}`);
  let codeOk = true;
  for (const c of [d.contracts.bound, d.contracts.token]) {
    const acc = (await withRetry(() =>
      p.viewAccount({ accountId: c.accountId, blockQuery: { finality: "final" } } as never),
    )) as { code_hash: string };
    const ok = acc.code_hash === c.codeHash;
    codeOk &&= ok;
    console.log(
      `  ${c.accountId} on-chain code_hash ${acc.code_hash} ${ok ? "==" : "!="} recorded ${c.codeHash} (wasm sha256 ${c.wasmSha256})`,
    );
  }
  const config = await view(p, B, "get_config");
  const accounts = await view(p, B, "get_accounts");
  const certs = await view<number>(p, B, "cert_count");
  const challenges = await view<number>(p, B, "challenge_count");
  console.log(`  get_config   ${JSON.stringify(config)}`);
  console.log(`  get_accounts ${JSON.stringify(accounts)}`);
  console.log(`  certificates ${certs}, challenges ${challenges}`);
  console.log(`  deploy txs:`);
  for (const t of d.txs) console.log(`    ${t.step}: ${explorerTx(t.hash)}`);
  for (const r of d.demoRuns) {
    console.log(
      `  demo run ${r.runId}: agent ${r.agent} (cert ${r.certId}), agent2 ${r.agent2} (cert ${r.cert2Id}), evidence ${r.evidence}`,
    );
  }
  if (!codeOk) process.exit(1);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
