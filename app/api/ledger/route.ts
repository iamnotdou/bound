// Ledger endpoint — the data source for the /control proof board.
//
//   GET /api/ledger
//   → one snapshot of every actor's USDC balance plus the contract state that
//     matters (reserve held vs claimed, auditor stake, cert status). The board
//     polls this and flashes the delta after each actor action, so value moving
//     between the five named accounts is visible, not narrated.
//
// All reads simulate — no signatures, safe to poll. Relative imports so
// scripts/routes-smoke.ts can load it directly.
import { bound } from "../../lib/bound-client";
import { toCertView } from "../../lib/cert-view";
import { accounts } from "../../lib/accounts";
import { formatUsdc } from "../../lib/config";

export const runtime = "nodejs";

export async function GET() {
  try {
    const operator = accounts.operator.publicKey();
    const agent = accounts.agent.publicKey();
    const auditor = accounts.auditor.publicKey();
    const counterparty = accounts.counterparty.publicKey();
    const challenger = accounts.challenger.publicKey();

    const [
      opBal,
      agentBal,
      audBal,
      cpBal,
      chalBal,
      reserve,
      stake,
      cert,
      certId,
    ] = await Promise.all([
      bound.usdcBalance(operator),
      bound.usdcBalance(agent),
      bound.usdcBalance(auditor),
      bound.usdcBalance(counterparty),
      bound.usdcBalance(challenger),
      bound.reserveBalance(),
      bound.auditorStake(auditor),
      bound.verifyCertificate(agent),
      bound.certIdForAgent(agent),
    ]);

    const view = toCertView(agent, cert, certId);

    return Response.json({
      accounts: [
        { role: "Operator", address: operator, usdc: formatUsdc(opBal) },
        { role: "Agent", address: agent, usdc: formatUsdc(agentBal) },
        { role: "Auditor", address: auditor, usdc: formatUsdc(audBal) },
        { role: "Counterparty", address: counterparty, usdc: formatUsdc(cpBal) },
        { role: "Challenger", address: challenger, usdc: formatUsdc(chalBal) },
      ],
      contracts: {
        reserveHeldUsd: formatUsdc(reserve),
        reserveClaimedUsd: view.reserveUsd, // what the cert claims vs what the vault holds
        auditorStakeUsd: formatUsdc(stake),
      },
      cert: view,
    });
  } catch (err) {
    return Response.json({ error: (err as Error).message }, { status: 502 });
  }
}
