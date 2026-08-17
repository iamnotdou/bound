// Build an unsigned (assembled) XDR for a connected wallet to sign. The wallet
// is the tx source and the require_auth address, so the returned envelope needs
// only the wallet's signature. No secret key is used here.
//
//   POST /api/tx/build  { action, address, params }  → { xdr } | 400
import { buildActionXdr, type WalletAction, type BuildParams } from "../../../lib/tx-build";

export const runtime = "nodejs";
export const maxDuration = 60;

const ACTIONS: WalletAction[] = ["stake", "attest", "publish", "deposit-fee", "pay", "challenge"];

export async function POST(req: Request) {
  let action: WalletAction;
  let address: string;
  let params: BuildParams;
  try {
    const body = await req.json();
    action = body?.action;
    address = (body?.address ?? "").trim();
    params = body?.params ?? {};
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }

  if (!ACTIONS.includes(action)) {
    return Response.json({ error: `unknown action: ${action}` }, { status: 400 });
  }

  try {
    const xdr = await buildActionXdr(action, address, params);
    return Response.json({ xdr });
  } catch (err) {
    return Response.json({ error: (err as Error).message }, { status: 400 });
  }
}
