// pnpm near:agent-mcp [runId] -- an agent spends through the bound-near MCP
// server with the same tool calls an LLM would make (no model runs here), on
// the Verified second cert of a recorded demo run. Needs the run's local demo keys (near/.demo-keys/<runId>.json,
// gitignored); writes near/evidence/mcp-agent-<runId>.{json,txt}.
//
// 1. If agent2 still holds a full-access key, it adds a function-call key
//    (receiver: the current Bound contract, methods ["pay"], 0.25 NEAR gas
//    allowance) and deletes the full-access key, so the second cert's agent
//    is confined like the demo agent.
// 2. An MCP client calls the tools with ONLY that function-call key loaded:
//    verify itself, pay 1 to the allowlisted service (succeeds), pay more than
//    the float (refused by the contract), pay an account off the allowlist
//    (refused), then check its keys and account code (confined).

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { KeyPair, actions, type KeyPairString } from "near-api-js";
import {
  MILLINEAR,
  accountFromKey,
  decodeOutcome,
  testnetProvider,
  view,
  withRetry,
} from "../src/client.js";
import { DEMO_KEYS_DIR, EVIDENCE_DIR, loadDeployment } from "../src/deployment.js";
import { keysConfined, type AccessKeyInfo, type CertViewJson } from "../src/format.js";
import { createNearMcpServer } from "../src/mcp.js";

type Keys = Record<string, { accountId: string; secretKey: string }>;

async function main() {
  const d = loadDeployment();
  const runId = process.argv[2] ?? d.demoRuns.at(-1)!.runId;
  const run = d.demoRuns.find((r) => r.runId === runId);
  if (!run) throw new Error(`no demo run ${runId} in the deployment record`);
  const B = d.contracts.bound.accountId;
  const keysPath = join(DEMO_KEYS_DIR, `${runId}.json`);
  const keys = JSON.parse(readFileSync(keysPath, "utf8")) as Keys;
  const provider = testnetProvider();
  const lines: string[] = [];
  const out = (s: string) => {
    lines.push(s);
    console.log(s);
  };
  const steps: Array<Record<string, unknown>> = [];
  let bad = 0;
  const expect = (label: string, ok: boolean, detail: unknown) => {
    if (!ok) bad++;
    steps.push({ label, pass: ok, detail });
    out(`  ${ok ? "OK  " : "FAIL"} ${label}`);
    out(`       ${JSON.stringify(detail)}`);
  };

  const agentId = run.agent2;
  const keyList = async () =>
    (
      (await withRetry(() =>
        provider.viewAccessKeyList({ accountId: agentId, finality: "final" } as never),
      )) as unknown as { keys: AccessKeyInfo[] }
    ).keys;

  out(`Bound on NEAR: agent ${agentId} spends through the bound-near MCP server`);
  out(`contract ${B}, cert ${run.cert2Id}, run ${runId}, ${new Date().toISOString()}`);

  // 1. confine agent2 if needed
  let before = await keyList();
  if (!keysConfined(before, B)) {
    if (!keys.agent2) throw new Error(`no agent2 key in ${keysPath}`);
    const full = accountFromKey(provider, agentId, keys.agent2.secretKey as KeyPairString);
    const fc = KeyPair.fromRandom("ed25519");
    keys.agent2Fc = { accountId: agentId, secretKey: fc.toString() };
    writeFileSync(keysPath, JSON.stringify(keys, null, 2) + "\n", { mode: 0o600 });
    const add = await full.signAndSendTransaction({
      receiverId: agentId,
      actions: [actions.addFunctionCallAccessKey(fc.getPublicKey(), B, ["pay"], 250n * MILLINEAR)],
      waitUntil: "FINAL",
      throwOnFailure: false,
    });
    const addO = decodeOutcome(add as never, agentId, agentId, "AddKey");
    expect(`agent2 adds a function-call key (${B}, [pay], 0.25 NEAR)`, addO.ok, {
      tx: addO.url,
      error: addO.error,
    });
    const fullPub = KeyPair.fromString(keys.agent2.secretKey as KeyPairString).getPublicKey();
    const del = await full.signAndSendTransaction({
      receiverId: agentId,
      actions: [actions.deleteKey(fullPub)],
      waitUntil: "FINAL",
      throwOnFailure: false,
    });
    const delO = decodeOutcome(del as never, agentId, agentId, "DeleteKey");
    expect("agent2 deletes its full-access key", delO.ok, { tx: delO.url, error: delO.error });
    before = await keyList();
  } else {
    out("  agent2 is already confined; skipping the key change");
  }
  if (!keys.agent2Fc)
    throw new Error("agent2 is confined but its function-call key is not in the key file");

  // 2. the MCP session, holding only the function-call key
  const cert = await view<CertViewJson>(provider, B, "get_cert", { cert_id: run.cert2Id });
  const service = cert.allowlist?.[0];
  if (!service) throw new Error("cert 2 has no allowlist to pay");
  const server = createNearMcpServer({
    provider,
    contractId: B,
    agentId,
    agentKey: keys.agent2Fc.secretKey,
  });
  const [a, b] = InMemoryTransport.createLinkedPair();
  await server.connect(a);
  const client = new Client({ name: "bound-agent", version: "0" });
  await client.connect(b);
  const call = async (name: string, args: Record<string, unknown>) => {
    const r = await client.callTool({ name, arguments: args });
    const t = (r.content as Array<{ text: string }>)[0].text;
    return r.isError ? { error: t } : (JSON.parse(t) as Record<string, unknown>);
  };

  const self = await call("near_verify_agent", { agent: agentId });
  expect("near_verify_agent(self) -> deal", self.deal === true, { reason: self.reason });
  const floatBefore = BigInt(cert.float);
  const ok = await call("near_pay", {
    cert_id: run.cert2Id,
    to: service,
    amount: "1",
    memo: "mcp tool call",
  });
  expect(`near_pay 1 to allowlisted ${service} succeeds`, ok.ok === true, {
    tx: ok.explorer,
    logs: ok.logs,
  });
  const over = await call("near_pay", { cert_id: run.cert2Id, to: service, amount: "500" });
  expect(
    "near_pay 500 (over the float) is refused by the contract",
    over.ok === false && /FloatExceeded/.test(String(over.error)),
    { tx: over.explorer, error: over.error },
  );
  const off = await call("near_pay", {
    cert_id: run.cert2Id,
    to: d.accounts.treasury,
    amount: "1",
  });
  expect(
    "near_pay to an account off the allowlist is refused",
    off.ok === false && /RecipientNotAllowed/.test(String(off.error)),
    { tx: off.explorer, error: off.error },
  );
  const k = await call("near_check_agent_keys", { agent: agentId });
  expect(
    "near_check_agent_keys(self) -> confined (pay-only keys AND no contract code, local or global)",
    k.confined === true && k.no_contract_code === true && k.global_contract === null,
    { keys: k.keys, code_hash: k.code_hash, global_contract: k.global_contract },
  );
  const after = await view<CertViewJson>(provider, B, "get_cert", { cert_id: run.cert2Id });
  expect("float dropped by exactly 1", floatBefore - BigInt(after.float) === 1_000_000n, {
    before: cert.float,
    after: after.float,
    spent: after.spent,
  });
  await client.close();

  const base = join(EVIDENCE_DIR, `mcp-agent-${runId}`);
  writeFileSync(
    `${base}.json`,
    JSON.stringify(
      {
        runId,
        agent: agentId,
        certId: run.cert2Id,
        contract: B,
        at: new Date().toISOString(),
        keysBefore: before,
        steps,
        pass: bad === 0,
      },
      null,
      2,
    ) + "\n",
  );
  out(bad ? `${bad} problem(s)` : "all MCP agent steps behaved as expected");
  writeFileSync(`${base}.txt`, lines.join("\n") + "\n");
  process.exit(bad ? 1 : 0);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
