// Framework-agnostic Bound tool definitions. Each tool is a description + a Zod
// raw shape (the param map) + an execute() that calls BoundClient. The MCP
// server here and the AI SDK loop in apps/dashboard both adapt these — one
// core, two mouths. This file is the single definition of each tool; the app
// imports it back from `@bound/mcp/tools` rather than owning a second copy.
//
// SERVER ONLY: execute() signs with secret keys held in the environment.
//
// Model A (pure bonding): there is NO spending cap. The agent pays whatever it
// is asked. Safety is economic — the certificate bounds the counterparty's loss,
// and a false vouch is slashed via challenge_certificate.
//
// v2 hangs two rails off that. The PaymentRouter meters what an enrolled agent
// spends, and its counter is the on-chain state a BoundExceeded proof is read
// from — routing is what turns a payment into evidence. The PremiumVault prices
// the auditor's risk and pays them yield for carrying it, so vouching is a
// business rather than a favour.
import { z } from "zod";
import { bound, type ProofType, agentFetch, getCertificate, usdc, formatUsdc } from "@bound/sdk";
import { accounts } from "./accounts";

export interface BoundTool {
  description: string;
  parameters: z.ZodRawShape;
  /**
   * Simulates against the chain, signs nothing, spends nothing. Surfaced to MCP
   * clients as `readOnlyHint` so a human is asked to approve only the calls
   * that actually move money.
   */
  readOnly?: boolean;
  execute: (args: any) => Promise<unknown>;
}

const SECONDS_PER_DAY = 86_400;

export const boundTools: Record<string, BoundTool> = {
  verify_agent_certificate: {
    description:
      "Check an AI agent's Bound Certificate before transacting with it. Returns whether it is valid, the bounded worst-case loss, the locked reserve, and the auditor's slashable stake.",
    readOnly: true,
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
    readOnly: true,
    parameters: {
      address: z.string().optional().describe("G... address; omit for the agent itself"),
    },
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
      const receipt = await bound.executePayment(accounts.agent, recipient, usdc(amountUsd));
      return {
        success: true,
        amountUsd,
        recipient,
        txHash: receipt.hash,
        // Whether this payment is on the record a challenger can read.
        routed: receipt.routed,
        certId: receipt.certId,
      };
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
        paid: paid
          ? { amountUsd: paid.amount, recipient: paid.recipient, txHash: paid.txHash }
          : null,
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
        outcome:
          after.status.tag === "Invalid"
            ? "FRAUD_PROVEN — auditor slashed, victim compensated"
            : "challenge did not invalidate",
      };
    },
  },

  // ---- PaymentRouter: the metered rail ---------------------------------------

  get_routing_status: {
    description:
      "Check whether an address's payments are metered. An address bound to a certificate in the PaymentRouter pays from a float the router holds in custody, and every payment lands on that certificate's spend counter. An unenrolled address pays over the raw USDC rail instead, which leaves no trace any challenger can read.",
    readOnly: true,
    parameters: {
      address: z.string().optional().describe("G... address; omit for the agent itself"),
    },
    execute: async ({ address }: { address?: string }) => {
      const addr = address ?? accounts.agent.publicKey();
      const certId = await bound.routedCertId(addr);
      return {
        address: addr,
        routed: certId !== null,
        certId,
        floatHeld: formatUsdc(await bound.routedBalance(addr)),
      };
    },
  },

  get_cert_meter: {
    description:
      "Read a certificate's meter: cumulative routed spend, the float the router holds for it, that float's ceiling, and whether the operator has halted it. Spend is GROSS FLOW, not loss — a certificate that has routed more than its bound has broken a covenant about its own conduct, it has not thereby cost anyone that much money. The bound is returned alongside so the two can be compared.",
    readOnly: true,
    parameters: { certId: z.number().describe("Certificate id to read") },
    execute: async ({ certId }: { certId: number }) => {
      const id = BigInt(certId);
      const [spent, float, floatCap, halted, cert] = await Promise.all([
        bound.spendForCert(id),
        bound.floatForCert(id),
        bound.floatCapForCert(id),
        bound.certHalted(id),
        getCertificate(certId),
      ]);
      return {
        certId,
        agent: cert?.agent ?? null,
        status: cert?.status ?? null,
        bound: cert?.boundUsd ?? null,
        spent: formatUsdc(spent),
        float: formatUsdc(float),
        floatCap: formatUsdc(floatCap),
        halted,
      };
    },
  },

  enroll_agent: {
    description:
      "Bind the agent to a certificate in the PaymentRouter and set that certificate's float cap. Both the operator and the agent authorize: enrolment attaches spend to the operator's certificate and subjects the agent to metering and to the operator's kill switch, so neither may conscript the other. A binding is permanent — an operator cannot walk an agent off a certificate whose counter is climbing.",
    parameters: {
      certId: z.number().describe("Certificate id to meter the agent against"),
      floatCapUsd: z
        .number()
        .describe("Ceiling on the float the router will hold — what a stolen agent key can reach"),
    },
    execute: async ({ certId, floatCapUsd }: { certId: number; floatCapUsd: number }) => {
      await bound.enrollAgent(accounts.operator, accounts.agent, BigInt(certId), usdc(floatCapUsd));
      return {
        success: true,
        agent: accounts.agent.publicKey(),
        certId,
        floatCapUsd,
        note: "the agent's payments are now metered on this certificate's spend counter",
      };
    },
  },

  fund_float: {
    description:
      "Move USDC into the PaymentRouter's custody, crediting the agent an equal routed balance to pay from. execute_payment tops this up on demand, so funding ahead of time is optional: float that is never idle is float that cannot be stolen.",
    parameters: {
      amountUsd: z.number().describe("Amount in USDC dollars to move into the router"),
    },
    execute: async ({ amountUsd }: { amountUsd: number }) => {
      const { hash } = await bound.fundFloat(accounts.agent, usdc(amountUsd));
      const addr = accounts.agent.publicKey();
      return {
        success: true,
        amountUsd,
        txHash: hash,
        floatHeld: formatUsdc(await bound.routedBalance(addr)),
      };
    },
  },

  halt_certificate: {
    description:
      "The operator's kill switch: stop every transfer, withdrawal and burn on a certificate. Signed by the operator, not the agent — this is the lever that exists precisely for the case where the agent's key is the problem.",
    parameters: { certId: z.number().describe("Certificate id to halt") },
    execute: async ({ certId }: { certId: number }) => {
      const { hash } = await bound.haltCert(accounts.operator, BigInt(certId));
      return { success: true, certId, halted: true, txHash: hash };
    },
  },

  resume_certificate: {
    description: "Lift an operator halt and let a certificate's payments flow again.",
    parameters: { certId: z.number().describe("Certificate id to resume") },
    execute: async ({ certId }: { certId: number }) => {
      const { hash } = await bound.resumeCert(accounts.operator, BigInt(certId));
      return { success: true, certId, halted: false, txHash: hash };
    },
  },

  // ---- PremiumVault: the economy ---------------------------------------------

  quote_premium: {
    description:
      "Price coverage: bound x duration x rate. Pass a certId to quote a published certificate from its own recorded terms, or boundUsd + durationDays to price a hypothetical one before publishing it.",
    readOnly: true,
    parameters: {
      certId: z.number().optional().describe("Published certificate id to price from its terms"),
      boundUsd: z.number().optional().describe("Hypothetical bound in USDC dollars"),
      durationDays: z.number().optional().describe("Hypothetical coverage length in days"),
    },
    execute: async ({
      certId,
      boundUsd,
      durationDays,
    }: {
      certId?: number;
      boundUsd?: number;
      durationDays?: number;
    }) => {
      if (certId !== undefined) {
        const premium = await bound.quotePremiumForCert(BigInt(certId));
        return { certId, premium: formatUsdc(premium) };
      }
      if (boundUsd === undefined || durationDays === undefined) {
        throw new Error("pass either certId, or both boundUsd and durationDays");
      }
      const premium = await bound.quotePremium(
        usdc(boundUsd),
        BigInt(Math.round(durationDays * SECONDS_PER_DAY)),
      );
      return { boundUsd, durationDays, premium: formatUsdc(premium) };
    },
  },

  get_coverage: {
    description:
      "Read a certificate's coverage: what the operator paid, the protocol's cut, and how much of the rest the auditor has earned so far. The auditor's yield accrues in a straight line across the term, and a slash forfeits only what is still unclaimed — the premium is yield on the stake, never a second bond. Returns the quote instead when no premium has been paid yet.",
    readOnly: true,
    parameters: { certId: z.number().describe("Certificate id to read coverage for") },
    execute: async ({ certId }: { certId: number }) => {
      const id = BigInt(certId);
      if (!(await bound.premiumPaid(id))) {
        return { certId, paid: false, quote: formatUsdc(await bound.quotePremiumForCert(id)) };
      }
      const [c, accrued, claimable] = await Promise.all([
        bound.coverage(id),
        bound.premiumAccrued(id),
        bound.premiumClaimable(id),
      ]);
      return {
        certId,
        paid: true,
        payer: c.payer,
        auditor: c.auditor,
        premium: formatUsdc(c.premium),
        protocolFee: formatUsdc(c.protocol_fee),
        yieldPot: formatUsdc(c.yield_pot),
        claimed: formatUsdc(c.claimed),
        accrued: formatUsdc(accrued),
        claimable: formatUsdc(claimable),
        startUnix: Number(c.start),
        durationSeconds: Number(c.duration),
        closed: c.closed,
        closedAtUnix: Number(c.closed_at),
      };
    },
  },

  pay_premium: {
    description:
      "Operator buys coverage for a certificate. Payable once, and only on a VERIFIED certificate — the premium is yield on the auditor's staked capital, and there is no auditor to pay until one has attested. The protocol's share leaves in the same transaction; the rest accrues to the auditor across the term.",
    parameters: { certId: z.number().describe("Certificate id to buy coverage for") },
    execute: async ({ certId }: { certId: number }) => {
      const id = BigInt(certId);
      const quote = await bound.quotePremiumForCert(id);
      const { hash } = await bound.payPremium(accounts.operator, id);
      return { success: true, certId, premium: formatUsdc(quote), txHash: hash };
    },
  },

  claim_premium: {
    description:
      "Auditor withdraws accrued-and-unclaimed yield. Allowed at any time, including mid-coverage: at every instant the accrued figure is payment for coverage already delivered. Claiming continuously converts forfeitable yield into settled income, which is deliberate — see get_coverage.",
    parameters: { certId: z.number().describe("Certificate id to claim yield from") },
    execute: async ({ certId }: { certId: number }) => {
      const { hash, result } = await bound.claimPremium(accounts.auditor, BigInt(certId));
      return { success: true, certId, claimed: formatUsdc(result), txHash: hash };
    },
  },
};
