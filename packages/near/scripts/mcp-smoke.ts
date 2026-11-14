// pnpm near:mcp-smoke -- zero-secret: connects an MCP client to the bound-near
// server in-process, lists the tools and calls the read tools against live
// testnet with the agents recorded in the deployment. Exit 1 on any mismatch.

import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import { createNearMcpServer } from "../src/mcp.js";
import { loadDeployment } from "../tooling/deployment.js";

async function main() {
  const run = loadDeployment().demoRuns.at(-1)!;
  const server = createNearMcpServer();
  const [a, b] = InMemoryTransport.createLinkedPair();
  await server.connect(a);
  const client = new Client({ name: "smoke", version: "0" });
  await client.connect(b);

  let bad = 0;
  const check = (label: string, ok: boolean, detail = "") => {
    if (!ok) bad++;
    console.log(`  ${ok ? "OK  " : "FAIL"} ${label}${detail ? ` (${detail})` : ""}`);
  };
  const call = async (name: string, args: Record<string, unknown>) => {
    const r = await client.callTool({ name, arguments: args });
    const t = (r.content as Array<{ text: string }>)[0].text;
    return { isError: r.isError === true, text: t, json: r.isError ? null : JSON.parse(t) };
  };

  const tools = (await client.listTools()).tools.map((t) => t.name).sort();
  check(
    "tools listed",
    tools.join(",") === "near_check_agent_keys,near_get_certificate,near_pay,near_verify_agent",
    tools.join(","),
  );

  const v2 = await call("near_verify_agent", { agent: run.agent2 });
  check(`near_verify_agent ${run.agent2} -> deal`, v2.json?.deal === true, v2.json?.reason);
  const v1 = await call("near_verify_agent", { agent: run.agent });
  check(`near_verify_agent ${run.agent} -> refuse`, v1.json?.deal === false, v1.json?.reason);
  const none = await call("near_verify_agent", { agent: "nobody-bound-smoke.testnet" });
  check("near_verify_agent unknown -> refuse", none.json?.deal === false, none.json?.reason);
  const cert = await call("near_get_certificate", { cert_id: run.certId });
  check(
    `near_get_certificate ${run.certId} settled`,
    cert.json?.settlement?.slash === "20000000" &&
      cert.json?.settlement?.slash_to_victims === "20000000",
    `status ${cert.json?.status}`,
  );
  const keys = await call("near_check_agent_keys", { agent: run.agent });
  check(
    `near_check_agent_keys ${run.agent} confined`,
    keys.json?.confined === true &&
      keys.json?.no_contract_code === true &&
      keys.json?.global_contract === null,
    `${keys.json?.keys?.length} key(s), code_hash ${keys.json?.code_hash}, global contract ${keys.json?.global_contract}`,
  );
  const pay = await call("near_pay", { cert_id: run.cert2Id, to: "x.testnet", amount: "1" });
  check("near_pay without a key is refused locally", pay.isError && /disabled/.test(pay.text));
  const badId = await call("near_verify_agent", { agent: "NOT..VALID" });
  check("invalid account id rejected by the schema", badId.isError);

  await client.close();
  console.log(bad ? `${bad} problem(s)` : "bound-near MCP smoke passed");
  process.exit(bad ? 1 : 0);
}
main().catch((e) => {
  console.error(e instanceof Error ? e.message : e);
  process.exit(1);
});
