// Coverage endpoint — the economics behind one certificate: what the
// PremiumVault charges for it and what the PaymentRouter has metered against it.
//
//   GET  /api/coverage?agent=G…
//     → CoverageView | 400 bad address | 502 chain read failed
//   POST /api/coverage { action: "pay-premium" | "claim-premium" | "enroll-agent" }
//     → the demo keys drive the operator and auditor sides of the economy.
//
// /api/verify already answers "is this certificate good?". This answers the two
// questions that come after it — "is it paid for?" and "what has it actually
// spent?" — and is a separate route because it reaches two more contracts and a
// caller who only wants the certificate should not pay for those round trips.
//
// Every field it returns is read from chain. Nothing here infers that a premium
// was paid, that yield accrued, or that an agent is metered: where a value
// cannot be read, the projection carries a null and the UI renders the absence.
//
// Uses relative imports (not the @ alias) so it stays directly loadable by
// scripts/routes-smoke.ts under plain ts-node.
import { bound, usdc, formatUsdc } from "@bound/sdk";
import { toCoverageView, type CoverageReadings } from "../../lib/coverage-view";
import { accounts } from "../../lib/accounts";

export const runtime = "nodejs";
export const maxDuration = 60;

const G_ADDRESS = /^G[A-Z2-7]{55}$/;

/** The float cap the demo enrolls its agent under. Small on purpose: the cap is
 *  the number that bounds what a stolen agent key can reach, and a demo that
 *  set it to the bound would be teaching the wrong habit. */
const DEMO_FLOAT_CAP = usdc(2_000);

/**
 * `float_cap` and `is_halted` read the router's per-certificate config, which
 * only exists once some agent has been enrolled — on a certificate with none
 * they abort. That is an answer, but it arrives as an indistinguishable RPC
 * error, so it is not safe to convert into the claim "this certificate is not
 * enrolled". The reading degrades to null and the panel shows a blank instead
 * of asserting something it could not actually read.
 */
async function optional<T>(read: Promise<T>): Promise<T | null> {
  try {
    return await read;
  } catch {
    return null;
  }
}

export async function GET(req: Request) {
  const agent = (new URL(req.url).searchParams.get("agent") ?? "").trim();
  if (!G_ADDRESS.test(agent)) {
    return Response.json(
      { error: "Enter a valid Stellar address (G… , 56 chars)." },
      { status: 400 },
    );
  }

  try {
    const [verified, certIdNum, agentCertId, routedBalance] = await Promise.all([
      bound.verifyCertificate(agent),
      bound.certIdForAgent(agent),
      bound.routedCertId(agent),
      optional(bound.routedBalance(agent)),
    ]);

    // Everything below this point needs a certificate to hang off. Without one
    // there is nothing to price and nothing to meter, and the projection says so
    // rather than reporting a pile of zeroes that read like real readings.
    if (certIdNum === null) {
      const readings: CoverageReadings = {
        agent,
        certId: null,
        status: verified.status.tag as CoverageReadings["status"],
        bound: verified.bound,
        expiresAtUnix: Number(verified.expires_at),
        quote: null,
        paid: false,
        coverage: null,
        accrued: 0n,
        claimable: 0n,
        meter: {
          agentCertId,
          spent: 0n,
          float: 0n,
          floatCap: null,
          halted: null,
          routedBalance,
        },
      };
      return Response.json(toCoverageView(readings));
    }

    const certId = BigInt(certIdNum);
    const [quote, paid, spent, float, floatCap, halted] = await Promise.all([
      bound.quotePremiumForCert(certId),
      bound.premiumPaid(certId),
      bound.spendForCert(certId),
      bound.floatForCert(certId),
      optional(bound.floatCapForCert(certId)),
      optional(bound.certHalted(certId)),
    ]);

    // The coverage record, the accrued total and the claimable balance are only
    // read when the vault says a premium was paid. `get_coverage` aborts on a
    // certificate with no coverage, and reporting a zero accrual for one that
    // was never bought would blur "nothing has accrued yet" into "nobody bought
    // this" — two different facts the panel has to keep apart.
    const [coverage, accrued, claimable] = paid
      ? await Promise.all([
          bound.coverage(certId),
          bound.premiumAccrued(certId),
          bound.premiumClaimable(certId),
        ])
      : [null, 0n, 0n];

    const readings: CoverageReadings = {
      agent,
      certId: certIdNum,
      status: verified.status.tag as CoverageReadings["status"],
      bound: verified.bound,
      expiresAtUnix: Number(verified.expires_at),
      quote,
      paid,
      coverage,
      accrued,
      claimable,
      meter: { agentCertId, spent, float, floatCap, halted, routedBalance },
    };
    return Response.json(toCoverageView(readings));
  } catch (err) {
    return Response.json(
      { error: `On-chain read failed: ${(err as Error).message}` },
      { status: 502 },
    );
  }
}

export async function POST(req: Request) {
  let action = "";
  try {
    action = (await req.json())?.action ?? "";
  } catch {
    return Response.json({ error: "invalid JSON body" }, { status: 400 });
  }

  const agent = accounts.agent.publicKey();
  let certId: bigint;
  try {
    const id = await bound.certIdForAgent(agent);
    if (id === null) {
      return Response.json(
        { error: "publish a certificate before buying coverage for it" },
        { status: 409 },
      );
    }
    certId = BigInt(id);
  } catch (err) {
    return Response.json({ error: (err as Error).message }, { status: 502 });
  }

  try {
    switch (action) {
      case "pay-premium": {
        // The vault authenticates against the certificate's own operator, read
        // live from the Registry, so this only succeeds while the demo operator
        // is the one named on it. A certificate published from a connected
        // wallet belongs to that wallet and reverts here — correctly.
        const { hash } = await bound.payPremium(accounts.operator, certId);
        const c = await bound.coverage(certId);
        return Response.json({
          action,
          hash,
          certId: Number(certId),
          premiumUsd: formatUsdc(c.premium),
          protocolFeeUsd: formatUsdc(c.protocol_fee),
          yieldPotUsd: formatUsdc(c.yield_pot),
        });
      }
      case "claim-premium": {
        const { hash, result } = await bound.claimPremium(accounts.auditor, certId);
        return Response.json({
          action,
          hash,
          certId: Number(certId),
          claimedUsd: formatUsdc(result),
        });
      }
      case "enroll-agent": {
        // Both keys sign: enrollment attaches the agent's spend to the
        // operator's certificate and puts the agent under the operator's kill
        // switch, and neither party may conscript the other. A binding is
        // permanent, so re-seeding the certificate and enrolling again reverts
        // with `already_enrolled` — the agent stays metered against the older
        // certificate, which is what keeps the counter usable as evidence.
        await bound.enrollAgent(accounts.operator, accounts.agent, certId, DEMO_FLOAT_CAP);
        return Response.json({
          action,
          certId: Number(certId),
          floatCapUsd: formatUsdc(DEMO_FLOAT_CAP),
        });
      }
      default:
        return Response.json({ error: `unknown action: ${action}` }, { status: 400 });
    }
  } catch (err) {
    return Response.json({ error: `${action} failed: ${(err as Error).message}` }, { status: 502 });
  }
}
