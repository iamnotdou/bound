// The tool table is a public contract in two directions at once: an MCP client
// reads the names and schemas off the wire, and apps/dashboard adapts the same
// objects onto the AI SDK. Both break silently on a malformed entry — MCP would
// advertise a tool with no input schema, the AI SDK would throw at z.object()
// — so this asserts the shape holds for every tool rather than for the handful
// a smoke run happens to call.
//
// Offline by construction: it inspects definitions and never invokes execute(),
// which is the only part that reaches the chain or touches a key.
import { describe, expect, it } from "vitest";
import { z } from "zod";
import { boundTools } from "./tools";

const READ_ONLY = [
  "verify_agent_certificate",
  "get_balance",
  "get_routing_status",
  "get_cert_meter",
  "quote_premium",
  "get_coverage",
];

const WRITES = [
  "execute_payment",
  "fetch_paid_service",
  "enroll_agent",
  "fund_float",
  "halt_certificate",
  "resume_certificate",
  "pay_premium",
  "claim_premium",
  "challenge_certificate",
];

describe("boundTools", () => {
  it("exposes exactly the documented tools", () => {
    expect(Object.keys(boundTools).sort()).toEqual([...READ_ONLY, ...WRITES].sort());
  });

  it.each(Object.entries(boundTools))("%s is a well-formed tool", (_name, tool) => {
    expect(tool.description.length).toBeGreaterThan(0);
    expect(typeof tool.execute).toBe("function");
    // z.object() over the raw shape is exactly what the AI SDK adapter does,
    // and what the MCP server hands to its own validator.
    expect(() => z.object(tool.parameters)).not.toThrow();
    for (const schema of Object.values(tool.parameters)) {
      expect(schema).toBeInstanceOf(z.ZodType);
    }
  });

  it("marks every non-mutating tool read-only, and no others", () => {
    const readOnly = Object.entries(boundTools)
      .filter(([, t]) => t.readOnly === true)
      .map(([name]) => name);
    expect(readOnly.sort()).toEqual([...READ_ONLY].sort());
  });
});
