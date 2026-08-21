// Adapts the Bound tool table onto an MCP server. Transport-free on purpose:
// the stdio wiring lives in bin.ts, so an embedder that wants Bound's tools
// inside its own server (HTTP, SSE, in-process) can take this and supply its
// own transport.
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { boundTools, type BoundTool } from "./tools";
import { version } from "../package.json";

/**
 * Reported over the wire, so a client can tell which connector it is talking
 * to. Read from package.json and inlined at build time rather than typed out
 * here: the literal it replaced carried a comment promising it was "kept in
 * step with package.json by hand", and it fell out of step on the very first
 * version bump. A constant that has to be remembered will eventually be
 * forgotten, and a connector reporting the wrong version of itself is worse
 * than one reporting none.
 */
export const VERSION: string = version;

/**
 * Build a server exposing every Bound tool.
 *
 * Results go back as pretty-printed JSON text rather than a structured output
 * schema: the tools return small, self-describing objects with money already
 * formatted as dollar strings, and a model reads those fine. A thrown error is
 * returned as an `isError` result instead of being allowed to escape — an MCP
 * client should see "that call failed and here is why" and be able to try
 * something else, not lose the session because a chain read timed out.
 */
export function createBoundMcpServer(tools: Record<string, BoundTool> = boundTools): McpServer {
  const server = new McpServer({ name: "bound-protocol", version: VERSION });

  for (const [name, t] of Object.entries(tools)) {
    server.registerTool(
      name,
      {
        description: t.description,
        inputSchema: t.parameters,
        // Only the read-only hint is claimed. `destructiveHint` and
        // `idempotentHint` would both be lies here: a payment is neither
        // reversible nor safe to replay.
        annotations: { readOnlyHint: t.readOnly === true },
      },
      async (args: unknown) => {
        try {
          const result = await t.execute(args);
          return { content: [{ type: "text" as const, text: JSON.stringify(result, null, 2) }] };
        } catch (err: unknown) {
          const message = err instanceof Error ? err.message : String(err);
          return {
            isError: true,
            content: [{ type: "text" as const, text: `Error: ${message}` }],
          };
        }
      },
    );
  }

  return server;
}
