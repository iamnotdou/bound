// Framework-agnostic Bound tool definitions. Each tool is a description + a Zod
// raw shape (the param map) + an execute() that calls BoundClient. The AI SDK
// loop and the MCP server both adapt these — one core, two mouths.
//
// SERVER ONLY: execute() signs with secret keys held in the environment.
//
// Model A (pure bonding): there is NO spending cap. The agent pays whatever it
// is asked. Safety is economic — the certificate bounds the counterparty's loss,
// and a false vouch is slashed via challenge_certificate.
import { z } from "zod";
import { bound, type ProofType } from "./bound-client";
import { agentFetch } from "./payments";
import { accounts } from "./accounts";
import { usdc, formatUsdc } from "./config";

export interface BoundTool {
  description: string;
  parameters: z.ZodRawShape;
  execute: (args: any) => Promise<unknown>;
}

export const boundTools: Record<string, BoundTool> = {
  verify_agent_certificate: {
    description:
      "Check an AI agent's Bound Certificate before transacting with it. Returns whether it is valid, the bounded worst-case loss, the locked reserve, and the auditor's slashable stake.",
    parameters: { agent: z.string().describe("The G... address of the agent to verify") },
    execute: async ({ agent }: { agent: string }) => {
      const v = await bound.verifyCertificate(agent);
      return {
        valid: v.valid,
        status: v.status.tag,
        bound: formatUsdc(v.bound),
        reserve: formatUsdc(v.reserve),
        auditorStake: formatUsdc(v.auditor_stake),
        auditor: v.auditor ?? null,
      };
    },
  },

  get_balance: {
    description: "Get the USDC balance of an address. Defaults to this agent's own balance.",
    parameters: { address: z.string().optional().describe("G... address; omit for the agent itself") },
    execute: async ({ address }: { address?: string }) => {
      const addr = address ?? accounts.agent.publicKey();
      return { address: addr, balance: formatUsdc(await bound.usdcBalance(addr)) };
    },
  },

  execute_payment: {
    description:
      "Send USDC directly to a recipient. The agent has no spending cap — it pays what it is asked. The counterparty's exposure is bounded by the agent's certificate, not by this call.",
    parameters: {
      recipient: z.string().describe("G... address to pay"),
      amountUsd: z.number().describe("Amount in USDC dollars"),
    },
    execute: async ({ recipient, amountUsd }: { recipient: string; amountUsd: number }) => {
      const hash = await bound.executePayment(accounts.agent, recipient, usdc(amountUsd));
      return { success: true, amountUsd, recipient, txHash: hash };
    },
  },

  fetch_paid_service: {
    description:
      "Fetch an HTTP resource that may require payment (x402). If the server answers 402, the agent autonomously pays the demanded price in USDC and retries. No human approves the amount — the service sets it, the agent decides.",
    parameters: { url: z.string().describe("URL of the paid service") },
    execute: async ({ url }: { url: string }) => {
      const { response, paid } = await agentFetch(url, accounts.agent, bound);
      const body = await response.text();
      return {
        status: response.status,
        paid: paid ? { amountUsd: paid.amount, recipient: paid.recipient, txHash: paid.txHash } : null,
        body,
      };
    },
  },

  challenge_certificate: {
    description:
      "Prove a certificate's attestation is false and trigger on-chain enforcement. For InsufficientReserve the contract verifies the fraud itself (claimed reserve > actual), then slashes the auditor's stake and compensates the victim. Returns the resulting certificate status.",
    parameters: {
      certId: z.number().describe("Certificate id to challenge"),
      victim: z.string().describe("G... address of the harmed counterparty to compensate"),
      proofType: z
        .enum(["InsufficientReserve", "BoundExceeded", "FakeSignature", "ExpiredCertificate"])
        .default("InsufficientReserve")
        .describe("Kind of fraud; only InsufficientReserve is verified trustlessly on-chain"),
    },
    execute: async ({
      certId,
      victim,
      proofType,
    }: {
      certId: number;
      victim: string;
      proofType?: ProofType["tag"];
    }) => {
      const before = await bound.verifyCertificate(accounts.agent.publicKey());
      const challengeId = await bound.challengeCertificate(accounts.challenger, {
        certId: BigInt(certId),
        proofType: (proofType ?? "InsufficientReserve") as ProofType["tag"],
        victim,
        bond: usdc(100),
      });
      const after = await bound.verifyCertificate(accounts.agent.publicKey());
      return {
        challengeId: Number(challengeId),
        certStatusBefore: before.status.tag,
        certStatusAfter: after.status.tag,
        outcome: after.status.tag === "Invalid" ? "FRAUD_PROVEN — auditor slashed, victim compensated" : "challenge did not invalidate",
      };
    },
  },
};
