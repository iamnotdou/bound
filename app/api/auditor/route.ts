// Auditor endpoint.
//
//   GET  → the pending certificate parameters the auditor is asked to vouch
//          for, plus the agent's current on-chain certificate state.
//   POST { action: "sign-publish" }
//        → operator publishes a fresh PENDING certificate and the auditor
//          immediately attests it (in the demo the auditor keypair signs
//          automatically). Result: a fresh VERIFIED certificate. This mutates
//          chain state and spends testnet fees.
//
// All signing happens here, server-side. Secret keys never reach the browser.
// Relative imports (not @ alias) so scripts/routes-smoke.ts can load it directly.
import { bound } from "../../lib/bound-client";
import { toCertView } from "../../lib/cert-view";
import { accounts } from "../../lib/accounts";
import { usdc, formatUsdc } from "../../lib/config";

export const runtime = "nodejs";
export const maxDuration = 60;

// The attestation the auditor is asked to stand behind. The reserve is the
// *claimed* coverage the auditor signs off on — the same lie the demo exposes
// (claimed reserve can exceed what the vault actually holds).
const BOUND = usdc(50_000);
const RESERVE_CLAIMED = usdc(10_000);
const AUDITOR_STAKE = usdc(1_500); // capital the auditor locks if not yet registered
const EXPIRY_DAYS = 30;

function expiresAt(): bigint {
  return BigInt(Math.floor(Date.now() / 1000) + EXPIRY_DAYS * 24 * 3600);
}

export async function GET() {
  const agent = accounts.agent.publicKey();
  try {
    const [current, stake, registered] = await Promise.all([
      bound.verifyCertificate(agent),
      bound.auditorStake(accounts.auditor.publicKey()),
      bound.auditorRegistered(accounts.auditor.publicKey()),
    ]);
    // The stake that will back the cert after signing: the auditor's current
    // locked capital, or AUDITOR_STAKE if they still need to register.
    const effectiveStake = registered ? stake : AUDITOR_STAKE;
    return Response.json({
      pending: {
        operator: accounts.operator.publicKey(),
        agent,
        auditor: accounts.auditor.publicKey(),
        boundUsd: formatUsdc(BOUND),
        reserveClaimedUsd: formatUsdc(RESERVE_CLAIMED),
        auditorStakeUsd: formatUsdc(effectiveStake),
        registered,
        willStakeOnSign: !registered,
        expiryDays: EXPIRY_DAYS,
      },
      current: toCertView(agent, current),
    });
  } catch (err) {
    return Response.json({ error: (err as Error).message }, { status: 502 });
  }
}

export async function POST(req: Request) {
  let action = "sign-publish";
  try {
    const body = await req.json();
    if (body?.action) action = body.action;
  } catch {
    /* default action */
  }

  if (action !== "sign-publish") {
    return Response.json({ error: `unknown action: ${action}` }, { status: 400 });
  }

  const agent = accounts.agent.publicKey();
  try {
    // 0 — ensure the auditor is registered. A prior demo may have slashed their
    // stake to zero; attest() requires a registered auditor, so re-stake first.
    const registered = await bound.auditorRegistered(accounts.auditor.publicKey());
    if (!registered) {
      await bound.stakeAsAuditor(accounts.auditor, AUDITOR_STAKE);
    }

    // 1 — operator publishes a PENDING certificate (claims the reserve)
    const certId = await bound.publishCertificate(accounts.operator, {
      agent,
      bound: BOUND,
      reserveAmount: RESERVE_CLAIMED,
      expiresAt: expiresAt(),
    });

    // 2 — auditor attests with their own staked capital → VERIFIED
    await bound.attestCertificate(accounts.auditor, certId);

    const cert = await bound.verifyCertificate(agent);
    return Response.json({ certId: Number(certId), cert: toCertView(agent, cert) });
  } catch (err) {
    return Response.json(
      { error: `Sign & publish failed: ${(err as Error).message}` },
      { status: 502 },
    );
  }
}
