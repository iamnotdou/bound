// Bound MCP server — exposes the same Bound tools to ANY MCP client
// (Claude Desktop, Cursor, third-party agents). One core (BoundClient),
// a second mouth alongside the in-app AI SDK loop.
//
// Run:   pnpm run mcp        (stdio transport)
// Wire into Claude Desktop / Cursor by pointing the MCP config at this command.
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { boundTools } from "../app/lib/agent-tools";

async function main() {
  const server = new McpServer({ name: "bound-protocol", version: "0.1.0" });

  for (const [name, t] of Object.entries(boundTools)) {
    server.tool(name, t.description, t.parameters, async (args: any) => {
      try {
        const result = await t.execute(args);
        return { content: [{ type: "text", text: JSON.stringify(result, null, 2) }] };
      } catch (err: any) {
        return {
          isError: true,
          content: [{ type: "text", text: `Error: ${err?.message ?? String(err)}` }],
        };
      }
    });
  }

  const transport = new StdioServerTransport();
  await server.connect(transport);
  // stderr so we don't corrupt the stdio JSON-RPC channel
  console.error("Bound MCP server running on stdio");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
