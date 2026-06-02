// x402 — let an agent autonomously pay for an HTTP resource. When a server
// answers 402 Payment Required, we read the price it demands, pay it through the
// USDC token (the same rail the rest of Bound uses), and retry with proof.
//
// The point for Bound: the *server* names the price, not the human. The agent
// decides to pay. Whether the demand is $10 or $10M, the agent's certificate
// bounds the counterparty's exposure — that's what makes autonomous payment safe.
import { Keypair } from "@stellar/stellar-sdk";
import { BoundClient } from "./bound-client";
import { usdc } from "./config";

// The 402 body shape this client understands.
export interface PaymentRequired {
  amount: number; // price in USDC dollars
  recipient: string; // G... address to pay
  asset?: string; // informational, expected "USDC"
}

export interface AgentFetchResult {
  response: Response;
  paid?: { amount: number; recipient: string; txHash?: string };
}

/**
 * Fetch `url`; if it returns 402, pay the demanded price via `signer` through
 * the SpendingLimit-free USDC rail, then retry once with an `X-Payment` proof.
 */
export async function agentFetch(
  url: string,
  signer: Keypair,
  bound: BoundClient,
  init?: RequestInit,
): Promise<AgentFetchResult> {
  const first = await fetch(url, init);
  if (first.status !== 402) return { response: first };

  const demand = (await first.json()) as PaymentRequired;
  if (!demand?.amount || !demand?.recipient) {
    throw new Error(`malformed 402 response: ${JSON.stringify(demand)}`);
  }

  const txHash = await bound.executePayment(signer, demand.recipient, usdc(demand.amount));

  const retry = await fetch(url, {
    ...init,
    headers: { ...(init?.headers ?? {}), "X-Payment": txHash ?? "" },
  });

  return {
    response: retry,
    paid: { amount: demand.amount, recipient: demand.recipient, txHash },
  };
}
