// A paid API that deals only with Bound-certified agents.
//
// Per request it reads `verify(agent)` and refuses anything that is not
// "Verified" (uncertified, Invalid, Expired, Challenged, halted) with 403.
// A certified agent is served either
//   (a) prepaid: the request carries the hash of a `pay` transaction the agent
//       signed, paying this service through the Bound router. The gate reads
//       the tx status from RPC, checks signer, receiver, success and the NEP-297
//       `pay` event, and accepts each hash once (replays refused, ledger file);
//   (b) on credit, up to `bound - outstanding invoices`, because a funded
//       reserve and a slashable auditor stand behind the agent. Unpaid
//       invoices are what a later Harm claim cites.
// Runs in-process (no port), so the demo and tests need no network binding.

import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { Provider } from "near-api-js";
import { view } from "./client.js";
import { assess, fmtUsdc, payEvents, type CertViewJson } from "./format.js";

export interface GateRequest {
  agent: string;
  /** Prepaid: hash of the agent's Bound `pay` tx to this service. */
  payTx?: string;
  /** On credit: amount in USDC base units. */
  credit?: bigint;
  description?: string;
}

export interface GateResponse {
  status: 200 | 402 | 403 | 409;
  reason: string;
  certId?: number;
  invoiceId?: string;
  paidAmount?: string;
}

export interface Invoice {
  id: string;
  agent: string;
  certId: number;
  amount: string;
  description: string;
  issuedAt: string;
  status: "unpaid" | "paid";
}

interface Files {
  ledger: string;
  invoices: string;
}

function readJson<T>(path: string, def: T): T {
  return existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")) as T) : def;
}
function writeJson(path: string, v: unknown) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(v, null, 2) + "\n");
}

export class Gate {
  constructor(
    private provider: Provider,
    private boundId: string,
    readonly serviceId: string,
    readonly price: bigint,
    private files: Files,
  ) {}

  async certOf(agent: string): Promise<CertViewJson | null> {
    return (
      (await view<CertViewJson | null>(this.provider, this.boundId, "verify", { agent })) ?? null
    );
  }

  invoices(): Invoice[] {
    return readJson<Invoice[]>(this.files.invoices, []);
  }

  async handle(req: GateRequest): Promise<GateResponse> {
    const cert = await this.certOf(req.agent);
    const a = assess(cert);
    if (!a.ok || !cert) return { status: 403, reason: a.reason };

    if (req.payTx) {
      const ledger = readJson<Record<string, string>>(this.files.ledger, {});
      if (ledger[req.payTx]) {
        return {
          status: 409,
          reason: `payment ${req.payTx} already used at ${ledger[req.payTx]}`,
          certId: cert.cert_id,
        };
      }
      const tx = (await this.provider.viewTransactionStatusWithReceipts({
        txHash: req.payTx,
        accountId: req.agent,
        waitUntil: "FINAL",
      } as never)) as unknown as {
        status: { SuccessValue?: string; Failure?: unknown };
        transaction: { signer_id: string; receiver_id: string };
        receipts_outcome: Array<{ outcome: { logs: string[] } }>;
      };
      if (tx.transaction.signer_id !== req.agent)
        return { status: 402, reason: "payment not signed by this agent" };
      if (tx.transaction.receiver_id !== this.boundId)
        return { status: 402, reason: "payment did not go through Bound" };
      if (!("SuccessValue" in tx.status))
        return { status: 402, reason: "payment transaction failed" };
      const logs = tx.receipts_outcome.flatMap((o) => o.outcome.logs);
      const paid = payEvents(logs).filter((e) => e.to === this.serviceId && e.agent === req.agent);
      const total = paid.reduce((s, e) => s + e.amount, 0n);
      if (total < this.price)
        return { status: 402, reason: `paid ${fmtUsdc(total)} < price ${fmtUsdc(this.price)}` };
      ledger[req.payTx] = new Date().toISOString();
      writeJson(this.files.ledger, ledger);
      return {
        status: 200,
        reason: "served (prepaid through Bound pay)",
        certId: cert.cert_id,
        paidAmount: fmtUsdc(total),
      };
    }

    if (req.credit !== undefined) {
      const outstanding = this.invoices()
        .filter((i) => i.agent === req.agent && i.certId === cert.cert_id && i.status === "unpaid")
        .reduce((s, i) => s + BigInt(i.amount), 0n);
      const limit = BigInt(cert.bound) - outstanding;
      if (req.credit > limit) {
        return {
          status: 402,
          reason: `credit ${fmtUsdc(req.credit)} exceeds remaining limit ${fmtUsdc(limit)}`,
        };
      }
      const all = this.invoices();
      const inv: Invoice = {
        id: `inv-${all.length + 1}`,
        agent: req.agent,
        certId: cert.cert_id,
        amount: req.credit.toString(),
        description: req.description ?? "service on credit",
        issuedAt: new Date().toISOString(),
        status: "unpaid",
      };
      writeJson(this.files.invoices, [...all, inv]);
      return {
        status: 200,
        reason: `served on credit (limit was ${fmtUsdc(limit)})`,
        certId: cert.cert_id,
        invoiceId: inv.id,
      };
    }

    return {
      status: 402,
      reason: `payment required: ${fmtUsdc(this.price)} USDC via Bound pay to ${this.serviceId}`,
    };
  }
}
