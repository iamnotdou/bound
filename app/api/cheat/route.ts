// Cheat endpoint — the adversarial proof lane of /control. Each action attempts
// a defection from the Nash table and is EXPECTED to revert. We only simulate
// (build the tx, never sign/submit), so nothing changes on-chain and no funds
// move — the revert reason itself is the proof that the cage holds.
//
//   POST { action: "withdraw-reserve" }  → expect `reserve_still_locked`
//   POST { action: "withdraw-stake" }    → expect `stake_locked`
//
// A reverted simulation is the SUCCESS case here: reverted=true + the reason.
// Relative imports so scripts/routes-smoke.ts can load it directly.
import { bound } from "../../lib/bound-client";
import { accounts } from "../../lib/accounts";

export const runtime = "nodejs";

const EXPECTED: Record<string, string> = {
  "withdraw-reserve": "reserve_still_locked",
  "withdraw-stake": "stake_locked",
};

export async function POST(req: Request) {
  let action = "";
  try {
    action = (await req.json())?.action ?? "";
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }

  if (!(action in EXPECTED)) {
    return Response.json({ error: `unknown action: ${action}` }, { status: 400 });
  }

  try {
    if (action === "withdraw-reserve") {
      await bound.simulateReleaseReserve(accounts.operator);
    } else {
      await bound.simulateReleaseStake(accounts.auditor);
    }
    // Reaching here means the lock did NOT hold — that is the failure case.
    return Response.json({
      action,
      reverted: false,
      warning: "defection simulated WITHOUT reverting — the lock did not hold",
    });
  } catch (err) {
    // Expected path: the contract trapped. Surface the reason as the proof.
    return Response.json({
      action,
      reverted: true,
      expected: EXPECTED[action],
      reason: (err as Error).message,
    });
  }
}
