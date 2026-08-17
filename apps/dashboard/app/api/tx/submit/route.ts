// Submit a wallet-signed envelope to the network, poll to completion, and return
// the tx hash + decoded return value (e.g. a challengeId for the challenger flow).
//
//   POST /api/tx/submit  { xdr }  → { hash, result } | 502
import { submitSignedXdr } from "../../../lib/tx-build";

export const runtime = "nodejs";
export const maxDuration = 60;

export async function POST(req: Request) {
  let xdr: string;
  try {
    xdr = (await req.json())?.xdr;
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }
  if (!xdr || typeof xdr !== "string") {
    return Response.json({ error: "signed xdr required" }, { status: 400 });
  }

  try {
    const out = await submitSignedXdr(xdr);
    return Response.json(out);
  } catch (err) {
    return Response.json({ error: `submit failed: ${(err as Error).message}` }, { status: 502 });
  }
}
