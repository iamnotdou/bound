import { describe, it, expect } from "vitest";
import { formatUsdcExact, ratio, toCoverageView, type CoverageReadings } from "./coverage-view";

const AGENT = "G" + "A".repeat(55);
const AUDITOR = "G" + "B".repeat(55);
const OPERATOR = "G" + "C".repeat(55);

const usd = (dollars: number) => BigInt(Math.round(dollars * 1e7));

function readings(over: Partial<CoverageReadings> = {}): CoverageReadings {
  return {
    agent: AGENT,
    certId: 7,
    status: "Verified",
    bound: usd(50_000),
    expiresAtUnix: 1_800_000_000,
    quote: usd(7.4),
    paid: false,
    coverage: null,
    accrued: 0n,
    claimable: 0n,
    meter: {
      agentCertId: null,
      spent: 0n,
      float: 0n,
      floatCap: null,
      halted: null,
      routedBalance: null,
    },
    ...over,
  };
}

function coverageRecord(over: Partial<NonNullable<CoverageReadings["coverage"]>> = {}) {
  return {
    payer: OPERATOR,
    auditor: AUDITOR,
    premium: usd(7.4),
    protocol_fee: usd(0.74),
    yield_pot: usd(6.66),
    claimed: 0n,
    start: 1_797_408_000n,
    duration: 2_592_000n, // 30 days
    closed: false,
    closed_at: 0n,
    ...over,
  };
}

describe("formatUsdcExact()", () => {
  it("keeps every significant stroop", () => {
    // The premium of a $1,500 bound at 200bps for 90 days, from the contract's
    // own worked example. Rounding this to three decimals — what the SDK's
    // headline formatter does — would drop most of it.
    expect(formatUsdcExact(73_972_602n)).toBe("$7.3972602");
    expect(formatUsdcExact(1n)).toBe("$0.0000001");
  });

  it("pads to two decimals but no further", () => {
    expect(formatUsdcExact(0n)).toBe("$0.00");
    expect(formatUsdcExact(usd(1_500))).toBe("$1,500.00");
    expect(formatUsdcExact(usd(0.5))).toBe("$0.50");
  });

  it("groups thousands and keeps a sign", () => {
    expect(formatUsdcExact(usd(1_234_567.89))).toBe("$1,234,567.89");
    expect(formatUsdcExact(-usd(12.5))).toBe("-$12.50");
  });
});

describe("ratio()", () => {
  it("returns null when there is nothing to divide by", () => {
    expect(ratio(usd(5), 0n)).toBeNull();
    expect(ratio(usd(5), -1n)).toBeNull();
  });

  it("does not clamp — the spend meter depends on seeing past 1", () => {
    expect(ratio(usd(75_000), usd(50_000))).toBe(1.5);
  });

  it("survives i128-scale numerators", () => {
    expect(ratio(usd(1) * 10n ** 6n, usd(4) * 10n ** 6n)).toBe(0.25);
  });
});

describe("toCoverageView() — premium state", () => {
  it("says there is nothing to price without a certificate", () => {
    const v = toCoverageView(readings({ certId: null, quote: null, status: "Pending" }));
    expect(v.premium.state).toBe("no-cert");
    expect(v.premium.quotedUsd).toBeNull();
    expect(v.premium.paid).toBe(false);
  });

  it("marks an unattested certificate as not for sale, but still priced", () => {
    const v = toCoverageView(readings({ status: "Pending" }));
    expect(v.premium.state).toBe("not-verified");
    expect(v.premium.quotedUsd).toBe("$7.40");
    expect(v.premium.record).toBeNull();
  });

  it("treats an invalidated certificate the same way", () => {
    expect(toCoverageView(readings({ status: "Invalid" })).premium.state).toBe("not-verified");
  });

  it("keeps a quote distinct from a payment", () => {
    const v = toCoverageView(readings());
    expect(v.premium.state).toBe("quoted");
    expect(v.premium.paid).toBe(false);
    // Nothing may stand in for an accrual that was never read from chain.
    expect(v.premium.record).toBeNull();
  });

  it("projects a live coverage record", () => {
    const v = toCoverageView(
      readings({
        paid: true,
        coverage: coverageRecord({ claimed: usd(1) }),
        accrued: usd(3.33),
        claimable: usd(2.33),
      }),
    );
    expect(v.premium.state).toBe("active");
    expect(v.premium.record).toMatchObject({
      auditor: AUDITOR,
      payer: OPERATOR,
      premiumUsd: "$7.40",
      protocolFeeUsd: "$0.74",
      yieldPotUsd: "$6.66",
      accruedUsd: "$3.33",
      claimedUsd: "$1.00",
      claimableUsd: "$2.33",
      termDays: 30,
      closed: false,
      closedAtIso: null,
    });
    expect(v.premium.record?.accruedFraction).toBeCloseTo(0.5, 3);
  });

  it("reports a closed coverage as closed, with the instant accrual stopped", () => {
    const v = toCoverageView(
      readings({
        paid: true,
        coverage: coverageRecord({ closed: true, closed_at: 1_798_000_000n, yield_pot: usd(1) }),
        accrued: usd(1),
        claimable: usd(1),
      }),
    );
    expect(v.premium.state).toBe("closed");
    expect(v.premium.record?.closedAtIso).toBe(new Date(1_798_000_000_000).toISOString());
  });
});

describe("toCoverageView() — spend meter", () => {
  it("reports an unenrolled agent as unenrolled", () => {
    const v = toCoverageView(readings());
    expect(v.meter.agentEnrolled).toBe(false);
    expect(v.meter.meteredHere).toBe(false);
    expect(v.meter.agentCertId).toBeNull();
  });

  it("distinguishes an agent metered against a different certificate", () => {
    const v = toCoverageView(
      readings({ certId: 9, meter: { ...readings().meter, agentCertId: 7 } }),
    );
    expect(v.meter.agentEnrolled).toBe(true);
    expect(v.meter.meteredHere).toBe(false);
    expect(v.meter.agentCertId).toBe(7);
  });

  it("flags routed spend past the bound without calling it a loss figure", () => {
    const v = toCoverageView(
      readings({ meter: { ...readings().meter, agentCertId: 7, spent: usd(75_000) } }),
    );
    expect(v.meter.overBound).toBe(true);
    expect(v.meter.spentFraction).toBe(1.5);
    expect(v.meter.spentUsd).toBe("$75,000.00");
  });

  it("does not call a zero bound an overrun", () => {
    const v = toCoverageView(readings({ bound: 0n }));
    expect(v.meter.overBound).toBe(false);
    expect(v.meter.spentFraction).toBeNull();
  });

  it("allows float above the cap, which inbound payments can produce", () => {
    const v = toCoverageView(
      readings({
        meter: { ...readings().meter, agentCertId: 7, float: usd(2_500), floatCap: usd(2_000) },
      }),
    );
    expect(v.meter.overFloatCap).toBe(true);
    expect(v.meter.floatCapUsd).toBe("$2,000.00");
  });

  it("leaves an unread float cap null rather than guessing zero", () => {
    const v = toCoverageView(readings());
    expect(v.meter.floatCapUsd).toBeNull();
    expect(v.meter.floatFraction).toBeNull();
    expect(v.meter.overFloatCap).toBe(false);
    expect(v.meter.halted).toBeNull();
  });
});
