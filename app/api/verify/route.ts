// Certificate lookup endpoint. Wraps BoundClient.verifyCertificate so a browser
// (or any client) can read a real testnet certificate without importing
// server-only code or handling bigints.
//
//   POST /api/verify   { "agent": "G..." }
//   → 200 CertView | 400 bad address | 502 chain read failed
//
// Uses relative imports (not the @ alias) so it is directly loadable by
// scripts/routes-smoke.ts under plain ts-node.
import { bound } from "../../lib/bound-client";
import { toCertView } from "../../lib/cert-view";

export const runtime = "nodejs";

const G_ADDRESS = /^G[A-Z2-7]{55}$/;

export async function POST(req: Request) {
  let agent: string;
  try {
    ({ agent } = await req.json());
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }

  agent = (agent ?? "").trim();
  if (!G_ADDRESS.test(agent)) {
    return Response.json(
      { error: "Enter a valid Stellar address (G… , 56 chars)." },
      { status: 400 },
    );
  }

  try {
    const result = await bound.verifyCertificate(agent);
    return Response.json(toCertView(agent, result));
  } catch (err) {
    return Response.json(
      { error: `On-chain read failed: ${(err as Error).message}` },
      { status: 502 },
    );
  }
}
