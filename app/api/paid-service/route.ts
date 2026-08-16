// Mock x402 paid service. Without payment it answers 402 with the price it
// demands; with an X-Payment proof header it returns the content.
//
// Query ?price=120 lets the demo vary how much the service charges — the point
// of Bound is that the agent pays whatever is asked, and the certificate (not
// this server) bounds the counterparty's exposure.
import { accounts } from "../../lib/config";

// The counterparty is the party selling the service, so it collects the fee.
const RECIPIENT = accounts.counterparty;

export async function GET(req: Request) {
  const url = new URL(req.url);
  const price = Number(url.searchParams.get("price") ?? "120");

  const payment = req.headers.get("x-payment");
  if (!payment) {
    return Response.json({ amount: price, recipient: RECIPIENT, asset: "USDC" }, { status: 402 });
  }

  // In a real service we'd verify `payment` (tx hash) settled the demanded
  // amount to RECIPIENT. For the demo we accept any proof and return content.
  return Response.json({
    service: "premium-market-data",
    pricePaidUsd: price,
    paymentRef: payment,
    data: "Premium market analysis: BTC regime shift detected; rotate 8% into majors.",
  });
}
