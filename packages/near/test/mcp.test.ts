import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { InMemoryTransport } from "@modelcontextprotocol/sdk/inMemory.js";
import type { Provider } from "near-api-js";
import { describe, expect, it } from "vitest";
import { NO_CODE_HASH } from "../src/format.js";
import { createNearMcpServer } from "../src/mcp.js";

const C = "bound-v3.x.testnet";
const payKey = {
  public_key: "ed25519:k",
  access_key: {
    nonce: 1,
    permission: { FunctionCall: { allowance: null, receiver_id: C, method_names: ["pay"] } },
  },
};

/** A provider that answers only the two views near_check_agent_keys reads. */
function fakeProvider(account: Record<string, unknown>): Provider {
  return {
    viewAccessKeyList: async () => ({ keys: [payKey] }),
    viewAccount: async () => ({ amount: "1", locked: "0", storage_usage: 1, ...account }),
  } as unknown as Provider;
}

async function checkKeys(account: Record<string, unknown>) {
  const server = createNearMcpServer({ provider: fakeProvider(account), contractId: C });
  const [a, b] = InMemoryTransport.createLinkedPair();
  await server.connect(a);
  const client = new Client({ name: "test", version: "0" });
  await client.connect(b);
  const r = (await client.callTool({
    name: "near_check_agent_keys",
    arguments: { agent: "agent.x.testnet" },
  })) as { content: Array<{ text: string }> };
  await client.close();
  return JSON.parse(r.content[0]!.text) as Record<string, unknown>;
}

describe("near_check_agent_keys", () => {
  it("pay-only keys, no code and no global contract: confined", async () => {
    const r = await checkKeys({ code_hash: NO_CODE_HASH });
    expect(r.confined).toBe(true);
    expect(r.no_contract_code).toBe(true);
    expect(r.global_contract).toBe(null);
  });
  it("a global contract by hash is not confined although code_hash is all ones", async () => {
    const r = await checkKeys({
      code_hash: NO_CODE_HASH,
      global_contract_hash: "8FstF2Tj8nHkco8AGLJ2gqfiAqdgaoTbgkpjaBLFCvJ",
    });
    expect(r.keys_confined).toBe(true);
    expect(r.no_local_code).toBe(true);
    expect(r.no_contract_code).toBe(false);
    expect(r.confined).toBe(false);
  });
  it("a global contract by account id is not confined", async () => {
    const r = await checkKeys({
      code_hash: NO_CODE_HASH,
      global_contract_account_id: "wallet-code.testnet",
    });
    expect(r.confined).toBe(false);
    expect(r.global_contract).toBe("wallet-code.testnet");
  });
  it("local contract code is not confined", async () => {
    const r = await checkKeys({ code_hash: "FstF2Tj8nHkco8AGLJ2gqfiAqdgaoTbgkpjaBLFCvJcD" });
    expect(r.confined).toBe(false);
  });
});
