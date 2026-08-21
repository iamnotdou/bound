// @bound/mcp — Bound Protocol's tools, packaged for any MCP-capable agent.
//
// The executable (`bound-mcp`, built from src/bin.ts) is what an MCP client
// launches. This entry point is for embedders: take `createBoundMcpServer` to
// mount Bound's tools on your own transport, or `boundTools` to drive them from
// a framework that is not MCP at all — the dashboard's AI SDK loop does exactly
// that, and imports `@bound/mcp/tools` so it never pulls the MCP server in.
//
// SERVER ONLY. Every write tool signs with a secret key from the environment.
export { boundTools, type BoundTool } from "./tools";
export { accounts } from "./accounts";
export { createBoundMcpServer, VERSION } from "./server";
