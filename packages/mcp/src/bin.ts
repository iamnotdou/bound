#!/usr/bin/env node
// Bound MCP server over stdio — the executable an MCP client launches.
//
// Run:   npx @bound/mcp
//
// stdout is the JSON-RPC channel and nothing else may touch it. Every diagnostic
// this process emits goes to stderr; a stray console.log here corrupts the
// stream and the client's only symptom is a server that "won't connect".
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { createBoundMcpServer, VERSION } from "./server";

async function main() {
  const server = createBoundMcpServer();
  await server.connect(new StdioServerTransport());
  console.error(`Bound MCP server ${VERSION} running on stdio`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
