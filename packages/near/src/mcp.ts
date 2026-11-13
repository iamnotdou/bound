// MCP tools for Bound on NEAR: what an LLM agent (or the service it pays)
// needs to act on a certificate. Read tools need nothing; `near_pay` needs
// the agent's function-call key in the environment, and that key can call
// only `pay` on the Bound contract, so a prompt-injected model holding it can
// at worst spend the float to allowlisted recipients.

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import type { Provider } from "near-api-js";
import { z } from "zod";
import { accountFromKey, codeHashOf, testnetProvider, tryCall, view, withRetry } from "./client.js";
import { loadDeployment } from "./deployment.js";
import {
  assess,
  coverageOf,
  fmtUsdc,
  isConfined,
  keysConfined,
  NO_CODE_HASH,
  usdc,
  type AccessKeyInfo,
  type CertViewJson,
} from "./format.js";

export interface NearToolEnv {
  provider?: Provider;
  contractId?: string;
  /** Agent account and its function-call private key (ed25519:...). */
  agentId?: string;
  agentKey?: string;
}

const accountId = z
  .string()
  .min(2)
  .max(64)
  .regex(/^(([a-z\d]+[-_])*[a-z\d]+\.)*([a-z\d]+[-_])*[a-z\d]+$/, "not a NEAR account id");

function money(v: CertViewJson) {
  return {
    cert_id: v.cert_id,
    status: v.status,
    agent: v.agent,
    operator: v.operator,
    auditor: v.auditor,
    bound: fmtUsdc(v.bound),
    reserve_locked: fmtUsdc(v.reserve_locked),
    reserve_promised: fmtUsdc(v.reserve_promised),
    auditor_allocation: fmtUsdc(v.allocation),
    coverage: fmtUsdc(coverageOf(v)),
    coverage_note:
      "reserve_locked + auditor allocation: the most arbitrated victims can recover in total, shared by every service extending credit",
    spent: fmtUsdc(v.spent),
    float: fmtUsdc(v.float),
    float_cap: fmtUsdc(v.float_cap),
    allowlist: v.allowlist,
    halted: v.halted,
    expires_at: new Date(v.expires_at * 1000).toISOString(),
    claim_window_closes_at: v.window_closes_at
      ? new Date(v.window_closes_at * 1000).toISOString()
      : null,
    settlement: v.settlement,
    unit: "mock USDC (testnet, 6 decimals)",
  };
}

export function createNearMcpServer(env: NearToolEnv = {}): McpServer {
  const provider = env.provider ?? testnetProvider();
  const contractId = env.contractId ?? loadDeployment().contracts.bound.accountId;
  const server = new McpServer({ name: "bound-near", version: "0.1.0" });
  const text = (o: unknown) => ({
    content: [{ type: "text" as const, text: JSON.stringify(o, null, 2) }],
  });
  const fail = (m: string) => ({
    isError: true,
    content: [{ type: "text" as const, text: `Error: ${m}` }],
  });
  const guard =
    <A>(fn: (a: A) => Promise<ReturnType<typeof text>>) =>
    async (a: A) => {
      try {
        return await fn(a);
      } catch (e) {
        return fail(e instanceof Error ? e.message : String(e));
      }
    };

  server.registerTool(
    "near_verify_agent",
    {
      description:
        "Before dealing with a NEAR agent account, read its Bound certificate: deal only if the result says deal=true (status Verified, not halted). Returns the reserve, auditor stake, coverage and spend meter behind the agent. Also call near_check_agent_keys: verify alone does not prove the agent is confined.",
      inputSchema: { agent: accountId },
      annotations: { readOnlyHint: true },
    },
    guard(async ({ agent }: { agent: string }) => {
      const v = await view<CertViewJson | null>(provider, contractId, "verify", { agent });
      const a = assess(v);
      return text({ agent, deal: a.ok, reason: a.reason, certificate: v ? money(v) : null });
    }),
  );

  server.registerTool(
    "near_get_certificate",
    {
      description: "Read one Bound certificate on NEAR by id.",
      inputSchema: { cert_id: z.number().int().min(0) },
      annotations: { readOnlyHint: true },
    },
    guard(async ({ cert_id }: { cert_id: number }) => {
      const v = await view<CertViewJson | null>(provider, contractId, "get_cert", { cert_id });
      if (!v) throw new Error(`no certificate ${cert_id}`);
      return text(money(v));
    }),
  );

  server.registerTool(
    "near_check_agent_keys",
    {
      description:
        "List a NEAR account's access keys and contract code hash and say whether it is confined: every key a function-call key for the Bound contract's pay method AND no contract code deployed on the account (code would run as the agent without a key), so NEAR itself refuses any other spend. Call this together with near_verify_agent: the Bound contract cannot check confinement itself.",
      inputSchema: { agent: accountId },
      annotations: { readOnlyHint: true },
    },
    guard(async ({ agent }: { agent: string }) => {
      const r = (await withRetry(() =>
        provider.viewAccessKeyList({
          accountId: agent,
          finalityQuery: { finality: "final" },
        } as never),
      )) as unknown as { keys: AccessKeyInfo[] };
      const codeHash = await codeHashOf(provider, agent);
      return text({
        agent,
        contract: contractId,
        confined: isConfined(r.keys, contractId, codeHash),
        keys_confined: keysConfined(r.keys, contractId),
        code_hash: codeHash,
        no_contract_code: codeHash === NO_CODE_HASH,
        keys: r.keys,
      });
    }),
  );

  server.registerTool(
    "near_pay",
    {
      description:
        "Pay a counterparty from this agent's Bound float through the metered pay method. Signed with the agent's function-call key (no deposit possible). The contract refuses amounts over the float, recipients off the operator's allowlist, and payments while halted or under an open claim window; the refusal text is returned.",
      inputSchema: {
        cert_id: z.number().int().min(0),
        to: accountId,
        amount: z.string().regex(/^\d+(\.\d{1,6})?$/, "decimal USDC amount, e.g. 2.5"),
        memo: z.string().max(64).optional(),
      },
    },
    guard(async (a: { cert_id: number; to: string; amount: string; memo?: string }) => {
      if (!env.agentId || !env.agentKey) {
        throw new Error(
          "near_pay is disabled: set BOUND_NEAR_AGENT_ID and BOUND_NEAR_AGENT_KEY (the agent's function-call key)",
        );
      }
      const acct = accountFromKey(provider, env.agentId, env.agentKey as never);
      const o = await tryCall(acct, contractId, "pay", {
        cert_id: a.cert_id,
        to: a.to,
        amount: usdc(a.amount).toString(),
        memo: a.memo ?? null,
      });
      return text({ ok: o.ok, tx: o.hash, explorer: o.url, error: o.error, logs: o.logs });
    }),
  );

  return server;
}
