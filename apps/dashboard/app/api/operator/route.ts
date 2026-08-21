// Operator endpoint — drives the Operator lane of the /control proof cockpit.
//
//   POST { action: "deposit-reserve", amountUsd?: number }  → fund the vault
//   POST { action: "deposit-fee" }                          → escrow the audit fee
//   POST { action: "publish" }                              → publish a PENDING cert
//
// The reserve amount is intentionally caller-controlled: fund it BELOW the
// claimed reserve ($10k) to set the trap the challenge later exposes, or fund
// it at/above to demonstrate the honest path (challenger forfeits the bond).
//
// All signing happens here with the operator key, server-side. Relative imports
// so scripts/routes-smoke.ts can load it directly.
import { bound, usdc, formatUsdc } from "@bound/sdk";
import { accounts } from "../../lib/accounts";

export const runtime = "nodejs";
export const maxDuration = 60;

// Same attestation parameters the auditor route stands behind.
const BOUND = usdc(50_000);
const RESERVE_CLAIMED = usdc(10_000);
const DEFAULT_RESERVE_DEPOSIT = 4_000; // the under-funded trap, in dollars
const EXPIRY_DAYS = 30;

function expiresAt(): bigint {
  return BigInt(Math.floor(Date.now() / 1000) + EXPIRY_DAYS * 24 * 3600);
}

export async function POST(req: Request) {
  let action = "";
  let amountUsd: number | undefined;
  try {
    const body = await req.json();
    action = body?.action ?? "";
    amountUsd = typeof body?.amountUsd === "number" ? body.amountUsd : undefined;
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }

  try {
    switch (action) {
      case "deposit-reserve": {
        const dollars = amountUsd ?? DEFAULT_RESERVE_DEPOSIT;
        // v2 keys reserves by certificate, so a deposit has to name one. The
        // vault reads this certificate's operator from the Registry and
        // authenticates against that, so only its own operator can fund it.
        const depositCertId = await bound.certIdForAgent(accounts.agent.publicKey());
        if (depositCertId === null) {
          return Response.json(
            { error: "publish a certificate before funding its reserve" },
            { status: 409 },
          );
        }
        const certId = BigInt(depositCertId);
        await bound.depositReserve(accounts.operator, certId, usdc(dollars));
        return Response.json({
          action,
          depositedUsd: formatUsdc(usdc(dollars)),
          reserveHeldUsd: formatUsdc(await bound.reserveBalance(certId)),
          claimedUsd: formatUsdc(RESERVE_CLAIMED),
        });
      }
      case "deposit-fee": {
        await bound.depositFee(accounts.operator, accounts.auditor.publicKey(), usdc(500));
        return Response.json({ action, feeUsd: formatUsdc(usdc(500)) });
      }
      case "publish": {
        // v2 authenticates the agent as well as the operator.
        const certId = await bound.publishCertificate(accounts.operator, accounts.agent, {
          bound: BOUND,
          reserveAmount: RESERVE_CLAIMED,
          expiresAt: expiresAt(),
        });
        return Response.json({
          action,
          certId: Number(certId),
          boundUsd: formatUsdc(BOUND),
          reserveClaimedUsd: formatUsdc(RESERVE_CLAIMED),
        });
      }
      default:
        return Response.json({ error: `unknown action: ${action}` }, { status: 400 });
    }
  } catch (err) {
    return Response.json({ error: `${action} failed: ${(err as Error).message}` }, { status: 502 });
  }
}
