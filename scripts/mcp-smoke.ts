// Spawn the Bound MCP server, list its tools, and call one read-only tool.
// Proves the MCP mouth works end-to-end (and reaches testnet through it).
//
// It launches the PACKAGED executable — the same `dist/bound-mcp.js` an MCP
// client would run after `npx @bound/mcp` — rather than the TypeScript source.
// Running the artefact is the point: a broken `bin`, a missing shebang or a
// dependency that only resolves inside this repo would all pass a source-level
// smoke test and fail on a stranger's machine.
import { existsSync } from "node:fs";
import { join } from "node:path";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";

const BIN = join(__dirname, "..", "packages", "mcp", "dist", "bound-mcp.js");

async function main() {
  if (!existsSync(BIN)) {
    throw new Error(`${BIN} not found — run \`pnpm build:mcp\` first`);
  }

  const transport = new StdioClientTransport({ command: process.execPath, args: [BIN] });
  const client = new Client({ name: "bound-smoke", version: "0.1.0" });
  await client.connect(transport);

  const { tools } = await client.listTools();
  console.log(`MCP tools exposed (${tools.length}):`, tools.map((t) => t.name).join(", "));

  const res: any = await client.callTool({ name: "get_balance", arguments: {} });
  console.log("get_balance via MCP:", res.content?.[0]?.text);

  const routing: any = await client.callTool({ name: "get_routing_status", arguments: {} });
  console.log("get_routing_status via MCP:", routing.content?.[0]?.text);

  await client.close();
  console.log("\n✓ MCP server works — any MCP client can drive Bound.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
