// Close a certificate's claim window, settling every admitted claim at once.
//
// v1 had a per-challenge `resolve` that settled the first claim immediately.
// That was the first-resolver-takes-all defect: one settlement drained the
// reserve and retired the allocation, foreclosing every honest claim behind it.
//
// v2 replaces it. A challenge opens (or joins) a 72-hour claim window and
// settles nothing; `close_window` settles the whole window once it lapses,
// paying admitted claims pro rata. It is permissionless and unpaid — the
// operator key here is just paying the fee.
//
//   POST /api/challenger/resolve  { certId }  → { closed, certId } | 409 | 502
import { bound } from "@bound/sdk";
import { accounts } from "../../../lib/accounts";

export const runtime = "nodejs";
export const maxDuration = 60;

export async function POST(req: Request) {
  let certId: number;
  try {
    certId = Number((await req.json())?.certId);
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }
  if (!Number.isInteger(certId) || certId < 1) {
    return Response.json({ error: "certId required" }, { status: 400 });
  }

  try {
    await bound.closeClaimWindow(accounts.operator, BigInt(certId));
    return Response.json({ closed: true, certId });
  } catch (err) {
    const message = (err as Error).message;
    // The commonest case by far: called before the window has lapsed.
    const early = /window_still_open|not_yet|deadline/i.test(message);
    return Response.json(
      { error: `close_window failed: ${message}` },
      { status: early ? 409 : 502 },
    );
  }
}
