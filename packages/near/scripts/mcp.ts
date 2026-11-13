// pnpm near:mcp -- Bound on NEAR as an MCP server over stdio.
// Read tools need no key. near_pay is enabled only when BOUND_NEAR_AGENT_ID
// and BOUND_NEAR_AGENT_KEY hold an agent account and its function-call key.
// stdout is the JSON-RPC channel: diagnostics go to stderr only.

import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { createNearMcpServer } from "../src/mcp.js";

async function main() {
  const server = createNearMcpServer({
    agentId: process.env.BOUND_NEAR_AGENT_ID,
    agentKey: process.env.BOUND_NEAR_AGENT_KEY,
  });
  await server.connect(new StdioServerTransport());
  console.error(
    `bound-near MCP on stdio (near_pay ${process.env.BOUND_NEAR_AGENT_KEY ? "enabled" : "disabled"})`,
  );
}
main().catch((e) => {
  console.error(e);
  process.exit(1);
});
