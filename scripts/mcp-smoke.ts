// Spawn the Bound MCP server, list its tools, and call one read-only tool.
// Proves the MCP mouth works end-to-end (and reaches testnet through it).
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

async function main() {
  const transport = new StdioClientTransport({
    command: "npx",
    args: ["ts-node", "--transpile-only", "--project", "scripts/tsconfig.json", "mcp/server.ts"],
  });
  const client = new Client({ name: "bound-smoke", version: "0.1.0" });
  await client.connect(transport);

  const { tools } = await client.listTools();
  console.log("MCP tools exposed:", tools.map((t) => t.name).join(", "));

  const res: any = await client.callTool({ name: "get_balance", arguments: {} });
  console.log("get_balance via MCP:", res.content?.[0]?.text);

  await client.close();
  console.log("\n✓ MCP server works — any MCP client can drive Bound.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
