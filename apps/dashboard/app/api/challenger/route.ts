// Challenger endpoint — drives the Challenger lane / the climax of the demo.
//
//   POST { certId?: number, victim?: string }
//   → posts a $100 bond and calls resolve(). For InsufficientReserve the
//     ChallengeManager proves the fraud itself (claimed reserve > actual vault
//     balance) and, in one tx, slashes the auditor's stake 80/20 (victim /
//     challenger), drains the reserve to the victim, and invalidates the cert.
//
// certId defaults to the live agent's mapped cert; victim defaults to the demo
// counterparty. Signs with the challenger key server-side — that secret never
// reaches the browser. Relative imports so routes-smoke can load it directly.
import { bound } from "../../lib/bound-client";
import { toCertView } from "../../lib/cert-view";
import { accounts } from "../../lib/accounts";
import { usdc, formatUsdc } from "../../lib/config";

export const runtime = "nodejs";
export const maxDuration = 60;

export async function POST(req: Request) {
  let certId: number | undefined;
  let victim: string | undefined;
  try {
    const body = await req.json().catch(() => ({}));
    certId = typeof body?.certId === "number" ? body.certId : undefined;
    victim = typeof body?.victim === "string" ? body.victim : undefined;
  } catch {
    /* defaults below */
  }

  const agent = accounts.agent.publicKey();
  victim = victim ?? accounts.counterparty.publicKey();

  try {
    if (certId == null) {
      const discovered = await bound.certIdForAgent(agent);
      if (discovered == null) {
        return Response.json({ error: "no certificate mapped to the agent" }, { status: 400 });
      }
      certId = discovered;
    }

    const [balBefore, certBefore] = await Promise.all([
      bound.usdcBalance(victim),
      bound.verifyCertificate(agent),
    ]);

    const challengeId = await bound.challengeCertificate(accounts.challenger, {
      certId: BigInt(certId),
      proofType: "InsufficientReserve",
      victim,
      bond: usdc(100),
    });

    const [balAfter, certAfter, stakeAfter, reserveAfter] = await Promise.all([
      bound.usdcBalance(victim),
      bound.verifyCertificate(agent),
      bound.auditorStake(accounts.auditor.publicKey()),
      bound.reserveBalance(),
    ]);

    const proven = certAfter.status.tag === "Invalid";
    return Response.json({
      challengeId: Number(challengeId),
      certId,
      victim,
      outcome: proven
        ? "FRAUD_PROVEN — auditor slashed, victim compensated, cert invalidated"
        : "no fraud found — challenger forfeits bond",
      victimUsdBefore: formatUsdc(balBefore),
      victimUsdAfter: formatUsdc(balAfter),
      auditorStakeAfterUsd: formatUsdc(stakeAfter),
      reserveAfterUsd: formatUsdc(reserveAfter),
      certBefore: toCertView(agent, certBefore, certId),
      certAfter: toCertView(agent, certAfter, certId),
    });
  } catch (err) {
    return Response.json({ error: `challenge failed: ${(err as Error).message}` }, { status: 502 });
  }
}
