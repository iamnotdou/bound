import { describe, it, expect } from "vitest";
import { usdc, formatUsdc, USDC_DECIMALS } from "./config";

// USDC on Stellar carries 7 decimals, so one dollar is 10^7 stroops.
const ONE_DOLLAR = 10_000_000n;

describe("usdc()", () => {
  it("uses 7 decimals", () => {
    expect(USDC_DECIMALS).toBe(7n);
  });

  it("converts whole dollars to stroops", () => {
    expect(usdc(0)).toBe(0n);
    expect(usdc(1)).toBe(ONE_DOLLAR);
    expect(usdc(500)).toBe(500n * ONE_DOLLAR);
    // The two figures the demo turns on: a $10,000 claimed bound against a
    // $4,000 funded reserve.
    expect(usdc(10_000)).toBe(100_000_000_000n);
    expect(usdc(4_000)).toBe(40_000_000_000n);
  });

  it("converts cents exactly", () => {
    expect(usdc(0.01)).toBe(100_000n);
    expect(usdc(0.99)).toBe(9_900_000n);
    expect(usdc(1.5)).toBe(15_000_000n);
    expect(usdc(1234.56)).toBe(12_345_600_000n);
  });

  it("returns a bigint, never a float", () => {
    expect(typeof usdc(1)).toBe("bigint");
  });

  it("handles negatives symmetrically", () => {
    expect(usdc(-1)).toBe(-ONE_DOLLAR);
    expect(usdc(-0.01)).toBe(-100_000n);
  });

  // The next two document real precision limits rather than endorsing them.
  // Both are consequences of `Math.round(dollars * 100)`: the cent is the
  // smallest representable unit, even though the chain stores 7 decimals.
  it("truncates anything smaller than a cent to zero", () => {
    expect(usdc(0.001)).toBe(0n);
    expect(usdc(0.004)).toBe(0n);
    // 0.005 rounds up to a full cent.
    expect(usdc(0.005)).toBe(100_000n);
  });

  it("rounds the half-cent boundary inconsistently, by binary floating point", () => {
    // `Math.round(dollars * 100)` inherits IEEE-754 error, so whether a
    // half-cent rounds up or down depends on the exact input rather than on any
    // rounding rule. These two look identical and are not:
    //
    //   1.005 * 100 -> 100.49999999999999  -> rounds DOWN to $1.00
    //   2.005 * 100 -> 200.5               -> rounds UP   to $2.01
    //
    // Locked in as a test because it is a property callers must not rely on:
    // pass amounts already rounded to the cent.
    expect(usdc(1.005)).toBe(100_000n * 100n); // $1.00, the half-cent is lost
    expect(usdc(2.005)).toBe(100_000n * 201n); // $2.01, the half-cent survives
    expect(usdc(8.165)).toBe(100_000n * 816n); // $8.16, lost
    expect(usdc(10.005)).toBe(100_000n * 1001n); // $10.01, survives
  });

  it("stays exact well past Number.MAX_SAFE_INTEGER in stroops", () => {
    // 10 billion dollars is 1e17 stroops, far beyond 2^53. The bigint return
    // type is what keeps this exact.
    expect(usdc(10_000_000_000)).toBe(100_000_000_000_000_000n);
    expect(usdc(10_000_000_000) > BigInt(Number.MAX_SAFE_INTEGER)).toBe(true);
  });
});

describe("formatUsdc()", () => {
  it("prefixes a dollar sign", () => {
    expect(formatUsdc(0n)).toBe("$0");
    expect(formatUsdc(ONE_DOLLAR)).toBe("$1");
  });

  it("divides stroops back down by 10^7", () => {
    expect(formatUsdc(500n * ONE_DOLLAR)).toBe("$500");
    expect(formatUsdc(100_000n)).toBe("$0.01");
  });

  it("round-trips whole and cent amounts through usdc()", () => {
    for (const dollars of [0, 1, 12, 500, 999]) {
      expect(formatUsdc(usdc(dollars))).toBe(`$${dollars}`);
    }
    expect(formatUsdc(usdc(0.5))).toBe("$0.5");
  });

  it("groups thousands the way the ambient locale does", () => {
    // formatUsdc calls toLocaleString() with no locale argument, so the
    // separator follows the host environment: "1,000" under en-US, "1.000"
    // under de-DE. Asserting against the same mechanism keeps this test
    // deterministic everywhere while still proving the division is right.
    expect(formatUsdc(usdc(10_000))).toBe(`$${(10_000).toLocaleString()}`);
    expect(formatUsdc(usdc(1_234.56))).toBe(`$${(1234.56).toLocaleString()}`);
  });

  it("loses precision above Number.MAX_SAFE_INTEGER", () => {
    // formatUsdc goes through Number(stroops), so display of very large
    // balances is approximate. This is a formatting concern only -- every
    // value that reaches a contract stays a bigint.
    const huge = usdc(10_000_000_000);
    expect(huge > BigInt(Number.MAX_SAFE_INTEGER)).toBe(true);
    expect(formatUsdc(huge)).toBe(`$${(1e10).toLocaleString()}`);
  });
});
