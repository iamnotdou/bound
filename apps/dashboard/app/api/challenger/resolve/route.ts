// Resolve a challenge that a connected-wallet challenger already opened (it
// signed `challenge` itself). `resolve` is permissionless, so we trigger the
// on-chain proof server-side — the finder's fee still routes to the wallet
// challenger recorded on the challenge. The operator key just pays the fee.
//
//   POST /api/challenger/resolve  { challengeId }  → { resolved, challengeId } | 502
import { bound } from "../../../lib/bound-client";
import { accounts } from "../../../lib/accounts";

export const runtime = "nodejs";
export const maxDuration = 60;

export async function POST(req: Request) {
  let challengeId: number;
  try {
    challengeId = Number((await req.json())?.challengeId);
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }
  if (!Number.isFinite(challengeId)) {
    return Response.json({ error: "challengeId required" }, { status: 400 });
  }

  try {
    await bound.resolveChallenge(accounts.operator, BigInt(challengeId));
    return Response.json({ resolved: true, challengeId });
  } catch (err) {
    return Response.json({ error: `resolve failed: ${(err as Error).message}` }, { status: 502 });
  }
}
