// A JSON-safe projection of one certificate's economics: the PremiumVault's
// coverage record, and the PaymentRouter's spend meter.
//
// It lives in the app rather than in @bound/sdk for the same reason CertView
// lives in the SDK — CertView is the shape every consumer of the protocol reads
// before transacting, while this is the shape one demo UI happens to want. The
// on-chain records carry i128 and u64 fields that cannot cross a JSON boundary,
// so the API route flattens them here.
//
// Unlike CertView, nothing in this module reaches the chain or touches
// server-only config, so both the types and the formatter are safe to import
// from a Client Component.
import type { CertStatusTag } from "@bound/sdk";

const STROOPS_PER_USD = 10_000_000n;
const SECONDS_PER_DAY = 86_400;

/**
 * Format USDC stroops exactly, without routing the value through a float.
 *
 * The SDK's `formatUsdc` is tuned for the protocol's headline figures —
 * thousands of dollars, where three decimal places is noise. Premium yield is
 * the opposite scale: a $7 pot accruing straight-line across a 30-day term is
 * worth fractions of a cent an hour, and `toLocaleString`'s default rounding
 * would render a live accrual as "$0". A panel that says an auditor has earned
 * nothing when the chain says otherwise is exactly the kind of claim this
 * surface must never make, so this keeps every significant stroop and pads only
 * up to the two decimals a reader expects to see.
 */
export function formatUsdcExact(stroops: bigint): string {
  const negative = stroops < 0n;
  const abs = negative ? -stroops : stroops;
  const whole = (abs / STROOPS_PER_USD).toLocaleString("en-US");
  const frac = (abs % STROOPS_PER_USD).toString().padStart(7, "0");
  const trimmed = frac.replace(/0+$/, "");
  const decimals = trimmed.length < 2 ? frac.slice(0, 2) : trimmed;
  return `${negative ? "-" : ""}$${whole}.${decimals}`;
}

/**
 * `numerator / denominator` as a plain number, or null when there is no
 * meaningful denominator. Divided in bigint hundredths of a percent first, so a
 * large i128 never loses its low digits on the way through Number.
 *
 * Deliberately uncapped. A spend fraction above 1 is the whole point of the
 * meter, and clamping it here would hide it from the caller.
 */
export function ratio(numerator: bigint, denominator: bigint): number | null {
  if (denominator <= 0n) return null;
  return Number((numerator * 10_000n) / denominator) / 10_000;
}

function iso(unixSeconds: bigint | number): string {
  return new Date(Number(unixSeconds) * 1000).toISOString();
}

/**
 * Where a certificate stands with the PremiumVault. Every one of these is read
 * from chain, never inferred from what the UI expects to be true.
 *
 * · `no-cert`      — nothing published for this agent, so nothing to price.
 * · `not-verified` — published, but `is_cert_verified` is false. `pay_premium`
 *                    refuses: the premium is yield on an auditor's staked
 *                    capital and there is no auditor yet (Pending) or no longer
 *                    one standing behind it (Invalid).
 * · `quoted`       — verified, and no Coverage record exists. Priced, not sold.
 * · `active`       — a Coverage record exists and is still accruing.
 * · `closed`       — a Coverage record exists and was closed by a settlement,
 *                    which froze accrual at that instant.
 */
export type PremiumState = "no-cert" | "not-verified" | "quoted" | "active" | "closed";

export interface CoverageRecordView {
  /** The operator who bought the coverage, snapshotted at payment. */
  payer: string;
  /** The auditor the yield accrues to, snapshotted at payment. */
  auditor: string;
  premiumUsd: string;
  /** The protocol's share. It left for the treasury in the paying transaction. */
  protocolFeeUsd: string;
  /** `premium - protocol_fee`: the ceiling on what this auditor can ever draw. */
  yieldPotUsd: string;
  accruedUsd: string;
  claimedUsd: string;
  claimableUsd: string;
  /** accrued / yield_pot — the share of the pot already earned. */
  accruedFraction: number | null;
  /** The certificate's issue date. Accrual starts here, not at the payment. */
  startIso: string;
  endIso: string;
  termDays: number;
  closed: boolean;
  closedAtIso: string | null;
}

export interface PremiumView {
  state: PremiumState;
  /** What the vault prices this certificate at, from its own immutable terms. */
  quotedUsd: string | null;
  /** True only when the vault holds a Coverage record for this certificate. */
  paid: boolean;
  /** The end of the priced window — the certificate's own expiry. */
  termEndsIso: string | null;
  record: CoverageRecordView | null;
}

export interface MeterView {
  /**
   * True when this agent's address is bound to some certificate in the router.
   * An unenrolled agent's transfers never reach the meter at all.
   */
  agentEnrolled: boolean;
  /**
   * The certificate the agent's spend is recorded against. An enrollment is
   * permanent, so an operator who republishes for the same agent leaves it
   * metered against the older certificate — which the panel has to say out
   * loud rather than quietly showing this certificate's counters.
   */
  agentCertId: number | null;
  /** agentCertId === the certificate being displayed. */
  meteredHere: boolean;
  /** Cumulative gross flow routed by this certificate. Not a measure of loss. */
  spentUsd: string;
  boundUsd: string;
  /** spent / bound, uncapped: above 1 is the BoundExceeded predicate. */
  spentFraction: number | null;
  overBound: boolean;
  /** Underlying USDC the router currently holds for this certificate. */
  floatUsd: string;
  /**
   * The deposit ceiling, or null when the router did not report one — either no
   * agent was ever enrolled on this certificate, or the read failed. The panel
   * renders the absence rather than guessing which.
   */
  floatCapUsd: string | null;
  floatFraction: number | null;
  /**
   * Float above the cap. Reachable and not a bug: the cap is checked on
   * `deposit`, while value arriving from another party raises the float without
   * a cap check, because refusing inbound payment would make an honest agent
   * unpayable.
   */
  overFloatCap: boolean;
  /** The operator's kill switch, or null when the router reported nothing. */
  halted: boolean | null;
  /** The agent's balance inside the router — not its USDC balance. */
  routedBalanceUsd: string | null;
}

export interface CoverageView {
  agent: string;
  certId: number | null;
  status: CertStatusTag;
  premium: PremiumView;
  meter: MeterView;
}

/** The raw on-chain readings the route gathers, before any of them are shaped. */
export interface CoverageReadings {
  agent: string;
  certId: number | null;
  status: CertStatusTag;
  bound: bigint;
  expiresAtUnix: number;
  /** `quote_cert`, or null when there is no certificate to price. */
  quote: bigint | null;
  /** `is_paid`. */
  paid: boolean;
  /** `get_coverage`, read only when `paid` — it aborts otherwise. */
  coverage: {
    payer: string;
    auditor: string;
    premium: bigint;
    protocol_fee: bigint;
    yield_pot: bigint;
    claimed: bigint;
    start: bigint;
    duration: bigint;
    closed: boolean;
    closed_at: bigint;
  } | null;
  accrued: bigint;
  claimable: bigint;
  meter: {
    agentCertId: number | null;
    spent: bigint;
    float: bigint;
    floatCap: bigint | null;
    halted: boolean | null;
    routedBalance: bigint | null;
  };
}

function premiumState(r: CoverageReadings): PremiumState {
  if (r.certId === null) return "no-cert";
  if (r.paid) return r.coverage?.closed ? "closed" : "active";
  return r.status === "Verified" ? "quoted" : "not-verified";
}

export function toCoverageView(r: CoverageReadings): CoverageView {
  const c = r.coverage;
  return {
    agent: r.agent,
    certId: r.certId,
    status: r.status,
    premium: {
      state: premiumState(r),
      quotedUsd: r.quote === null ? null : formatUsdcExact(r.quote),
      paid: r.paid,
      termEndsIso: r.expiresAtUnix > 0 ? iso(r.expiresAtUnix) : null,
      record: c
        ? {
            payer: c.payer,
            auditor: c.auditor,
            premiumUsd: formatUsdcExact(c.premium),
            protocolFeeUsd: formatUsdcExact(c.protocol_fee),
            yieldPotUsd: formatUsdcExact(c.yield_pot),
            accruedUsd: formatUsdcExact(r.accrued),
            claimedUsd: formatUsdcExact(c.claimed),
            claimableUsd: formatUsdcExact(r.claimable),
            accruedFraction: ratio(r.accrued, c.yield_pot),
            startIso: iso(c.start),
            endIso: iso(c.start + c.duration),
            termDays: Number(c.duration) / SECONDS_PER_DAY,
            closed: c.closed,
            closedAtIso: c.closed ? iso(c.closed_at) : null,
          }
        : null,
    },
    meter: {
      agentEnrolled: r.meter.agentCertId !== null,
      agentCertId: r.meter.agentCertId,
      meteredHere: r.certId !== null && r.meter.agentCertId === r.certId,
      spentUsd: formatUsdcExact(r.meter.spent),
      boundUsd: formatUsdcExact(r.bound),
      spentFraction: ratio(r.meter.spent, r.bound),
      overBound: r.bound > 0n && r.meter.spent > r.bound,
      floatUsd: formatUsdcExact(r.meter.float),
      floatCapUsd: r.meter.floatCap === null ? null : formatUsdcExact(r.meter.floatCap),
      floatFraction: r.meter.floatCap === null ? null : ratio(r.meter.float, r.meter.floatCap),
      overFloatCap: r.meter.floatCap !== null && r.meter.float > r.meter.floatCap,
      halted: r.meter.halted,
      routedBalanceUsd:
        r.meter.routedBalance === null ? null : formatUsdcExact(r.meter.routedBalance),
    },
  };
}
