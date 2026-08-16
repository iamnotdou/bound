import { describe, it, expect } from "vitest";
import { toCertView } from "./cert-view";
import type { VerifyResult } from "./bound-client";

const AGENT = "GCPOBMCWPO5A24KJJJRD27T4TKITHQI5MYY2FCQRR3HUXUFT4LO473ZT";
const AUDITOR = "GCBVHBXWW7CIFKZSGHOZIRYUYIS6S455EW64QK4FXZZ7WVW6R2FZN523";

function verifyResult(over: Partial<VerifyResult> = {}): VerifyResult {
  return {
    valid: true,
    status: { tag: "Verified", values: undefined },
    bound: 100_000_000_000n, // $10,000
    reserve: 40_000_000_000n, // $4,000
    auditor_stake: 50_000_000_000n, // $5,000
    auditor: AUDITOR,
    expires_at: 1_800_000_000n,
    ...over,
  } as VerifyResult;
}

describe("toCertView()", () => {
  it("projects i128 amounts into display strings", () => {
    const v = toCertView(AGENT, verifyResult());
    expect(v.boundUsd).toBe(`$${(10_000).toLocaleString()}`);
    expect(v.reserveUsd).toBe(`$${(4_000).toLocaleString()}`);
    expect(v.auditorStakeUsd).toBe(`$${(5_000).toLocaleString()}`);
  });

  it("is JSON-safe -- no bigint survives the projection", () => {
    const v = toCertView(AGENT, verifyResult());
    // The whole reason this function exists: i128 fields cannot cross a JSON
    // boundary, so serialising the view must not throw.
    expect(() => JSON.stringify(v)).not.toThrow();
    for (const value of Object.values(v)) {
      expect(typeof value).not.toBe("bigint");
    }
  });

  it("carries agent, validity and status through unchanged", () => {
    const v = toCertView(AGENT, verifyResult({ valid: false }));
    expect(v.agent).toBe(AGENT);
    expect(v.valid).toBe(false);
    expect(v.status).toBe("Verified");
  });

  it("reports each status tag", () => {
    for (const tag of ["Pending", "Verified", "Invalid"] as const) {
      const v = toCertView(AGENT, verifyResult({ status: { tag } } as Partial<VerifyResult>));
      expect(v.status).toBe(tag);
    }
  });

  describe("hasCert", () => {
    it("is true when a bound is set", () => {
      expect(toCertView(AGENT, verifyResult({ auditor: undefined })).hasCert).toBe(true);
    });

    it("is true when an auditor is set even with a zero bound", () => {
      expect(toCertView(AGENT, verifyResult({ bound: 0n })).hasCert).toBe(true);
    });

    it("is false only when the registry knows nothing about the agent", () => {
      const empty = verifyResult({ bound: 0n, auditor: undefined });
      expect(toCertView(AGENT, empty).hasCert).toBe(false);
    });
  });

  describe("expiry", () => {
    it("converts the unix seconds to an ISO timestamp", () => {
      const v = toCertView(AGENT, verifyResult({ expires_at: 1_800_000_000n }));
      expect(v.expiresAtUnix).toBe(1_800_000_000);
      // Seconds, not milliseconds -- the x1000 is the bug this guards.
      expect(v.expiresAtIso).toBe(new Date(1_800_000_000 * 1000).toISOString());
    });

    it("returns null rather than epoch when there is no expiry", () => {
      const v = toCertView(AGENT, verifyResult({ expires_at: 0n }));
      expect(v.expiresAtUnix).toBe(0);
      expect(v.expiresAtIso).toBeNull();
    });
  });

  describe("certId", () => {
    it("defaults to null", () => {
      expect(toCertView(AGENT, verifyResult()).certId).toBeNull();
    });

    it("passes a supplied id through, including zero", () => {
      expect(toCertView(AGENT, verifyResult(), 7).certId).toBe(7);
      // 0 is a legitimate cert id and must not be coerced to null.
      expect(toCertView(AGENT, verifyResult(), 0).certId).toBe(0);
    });
  });

  it("normalises a missing auditor to null", () => {
    expect(toCertView(AGENT, verifyResult({ auditor: undefined })).auditor).toBeNull();
    expect(toCertView(AGENT, verifyResult()).auditor).toBe(AUDITOR);
  });
});
