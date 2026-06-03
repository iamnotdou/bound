// Funding helper for a connected wallet, so a judge can go from "just connected"
// to "ready to stake / bond / pay" without manual setup. Three steps the client
// runs in order:
//
//   POST { address, step: "xlm" }        → friendbot funds XLM (creates account)
//   POST { address, step: "trustline" }  → returns an unsigned changeTrust XDR
//                                          (wallet signs, submits via /api/tx/submit)
//   POST { address, step: "mint", amountUsd? } → operator (issuer) mints test USDC
//
// Only the mint is signed server-side (operator is the USDC issuer). XLM is
// permissionless via friendbot; the trustline is wallet-signed.
import { bound } from "../../lib/bound-client";
import { accounts } from "../../lib/accounts";
import { usdc, formatUsdc } from "../../lib/config";
import { buildTrustlineXdr } from "../../lib/tx-build";

export const runtime = "nodejs";
export const maxDuration = 60;

const G_ADDRESS = /^G[A-Z2-7]{55}$/;
const FRIENDBOT = "https://friendbot.stellar.org";
const DEFAULT_MINT = 5_000;

export async function POST(req: Request) {
  let address = "";
  let step = "";
  let amountUsd: number | undefined;
  try {
    const body = await req.json();
    address = (body?.address ?? "").trim();
    step = body?.step ?? "";
    amountUsd = typeof body?.amountUsd === "number" ? body.amountUsd : undefined;
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }

  if (!G_ADDRESS.test(address)) {
    return Response.json({ error: "valid G… wallet address required" }, { status: 400 });
  }

  try {
    switch (step) {
      case "xlm": {
        const res = await fetch(`${FRIENDBOT}/?addr=${encodeURIComponent(address)}`);
        if (res.ok) return Response.json({ step, funded: true });
        const text = await res.text();
        if (/op_already_exists|already.*funded|createAccountAlreadyExist/i.test(text)) {
          return Response.json({ step, funded: true, alreadyFunded: true });
        }
        return Response.json({ error: `friendbot failed: ${text}` }, { status: 502 });
      }
      case "trustline": {
        const xdr = await buildTrustlineXdr(address);
        return Response.json({ step, xdr });
      }
      case "mint": {
        const dollars = amountUsd ?? DEFAULT_MINT;
        await bound.mintUsdc(accounts.operator, address, usdc(dollars));
        return Response.json({
          step,
          mintedUsd: formatUsdc(usdc(dollars)),
          balanceUsd: formatUsdc(await bound.usdcBalance(address)),
        });
      }
      default:
        return Response.json({ error: `unknown step: ${step}` }, { status: 400 });
    }
  } catch (err) {
    return Response.json({ error: `${step || "fund"} failed: ${(err as Error).message}` }, { status: 502 });
  }
}
