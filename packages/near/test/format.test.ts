import { describe, expect, it } from "vitest";
import {
  assess,
  describeCert,
  fmtUsdc,
  parseEvents,
  payEvents,
  usdc,
  type CertViewJson,
} from "../src/format.js";
import { decodeOutcome, panicText } from "../src/client.js";

const view = (over: Partial<CertViewJson> = {}): CertViewJson => ({
  cert_id: 0,
  status: "Verified",
  operator: "op.testnet",
  agent: "agent.testnet",
  auditor: "aud.testnet",
  bound: "150000000",
  reserve_promised: "100000000",
  reserve_locked: "100000000",
  allocation: "200000000",
  premium: "0",
  float: "20000000",
  float_cap: "20000000",
  spent: "0",
  post_expiry_spent: "0",
  pay_count: 0,
  allowlist: null,
  expires_at: 1_800_000_000,
  settlement_deadline: 1_800_000_300,
  halted: false,
  window_closes_at: null,
  open_claims: 0,
  settlement: null,
  now: 1_799_000_000,
  ...over,
});

describe("amounts", () => {
  it("parses and formats USDC with 6 decimals", () => {
    expect(usdc("12.5")).toBe(12_500_000n);
    expect(usdc("0.000001")).toBe(1n);
    expect(usdc(150)).toBe(150_000_000n);
    expect(fmtUsdc(12_500_000n)).toBe("12.5");
    expect(fmtUsdc("115000000")).toBe("115");
    expect(fmtUsdc(-300_000n)).toBe("-0.3");
  });
  it("rejects malformed amounts", () => {
    expect(() => usdc("1.0000001")).toThrow();
    expect(() => usdc("-1")).toThrow();
    expect(() => usdc("abc")).toThrow();
  });
});

describe("events", () => {
  const log =
    'EVENT_JSON:{"standard":"bound","version":"1.0.0","event":"pay","data":[{"cert_id":3,"agent":"a.testnet","to":"s.testnet","amount":"5000000","memo":"m","spent":"5000000"}]}';
  it("parses bound NEP-297 events and ignores others", () => {
    const ev = parseEvents([
      log,
      "plain log",
      'EVENT_JSON:{"standard":"nep141","event":"ft_transfer","data":[]}',
      "EVENT_JSON:{bad",
    ]);
    expect(ev).toHaveLength(1);
    expect(ev[0].event).toBe("pay");
  });
  it("decodes pay events", () => {
    expect(payEvents([log])).toEqual([
      { cert_id: 3, agent: "a.testnet", to: "s.testnet", amount: 5_000_000n, memo: "m" },
    ]);
  });
});

describe("cert views", () => {
  it("deals only with Verified, unhalted certs", () => {
    expect(assess(view()).ok).toBe(true);
    expect(assess(null)).toEqual({ ok: false, reason: "no Bound certificate for this agent" });
    expect(assess(view({ status: "Expired" })).ok).toBe(false);
    expect(assess(view({ status: "Challenged" })).reason).toContain("Challenged");
    expect(assess(view({ status: "Invalid" })).ok).toBe(false);
    expect(assess(view({ halted: true })).ok).toBe(false);
  });
  it("follows the contract's deal flag when present", () => {
    expect(assess(view({ deal: true })).ok).toBe(true);
    expect(assess(view({ deal: false, float: "0" }))).toEqual({
      ok: false,
      reason: "certificate has no float to pay from",
    });
  });
  it("describes a cert in one line", () => {
    expect(describeCert(view())).toContain("reserve 100/100");
  });
});

describe("tx outcomes", () => {
  it("extracts the contract panic message", () => {
    const failure = {
      ActionError: {
        kind: {
          FunctionCallError: {
            ExecutionError: "Smart contract panicked: FloatExceeded: amount 50 > float 15",
          },
        },
      },
    };
    expect(panicText(failure)).toBe("FloatExceeded: amount 50 > float 15");
    const o = decodeOutcome(
      { status: { Failure: failure }, transaction: { hash: "H" } },
      "a",
      "b",
      "pay",
    );
    expect(o.ok).toBe(false);
    expect(o.url).toBe("https://testnet.nearblocks.io/txns/H");
  });
  it("decodes a JSON SuccessValue", () => {
    const o = decodeOutcome(
      { status: { SuccessValue: Buffer.from("7").toString("base64") }, transaction: { hash: "H" } },
      "a",
      "b",
      "publish",
    );
    expect(o.ok).toBe(true);
    expect(o.value).toBe(7);
  });
});

describe("keysConfined / isConfined", () => {
  const C = "bound-v1.x.testnet";
  const fc = (receiver_id: string, method_names: string[]) => ({
    public_key: "ed25519:k",
    access_key: {
      nonce: 1,
      permission: { FunctionCall: { allowance: null, receiver_id, method_names } },
    },
  });
  it("only pay keys on the Bound contract count as confined", async () => {
    const { keysConfined: isConfined } = await import("../src/format.js");
    expect(isConfined([fc(C, ["pay"])], C)).toBe(true);
    expect(isConfined([], C)).toBe(false);
    expect(isConfined([fc(C, ["pay", "withdraw"])], C)).toBe(false);
    expect(isConfined([fc("usdc.x.testnet", ["pay"])], C)).toBe(false);
    expect(
      isConfined(
        [
          fc(C, ["pay"]),
          { public_key: "ed25519:f", access_key: { nonce: 1, permission: "FullAccess" } },
        ],
        C,
      ),
    ).toBe(false);
  });
  it("contract code on the agent account breaks confinement", async () => {
    const { isConfined, NO_CODE_HASH } = await import("../src/format.js");
    const keys = [fc(C, ["pay"])];
    expect(isConfined(keys, C, NO_CODE_HASH)).toBe(true);
    expect(isConfined(keys, C, "FstF2Tj8nHkco8AGLJ2gqfiAqdgaoTbgkpjaBLFCvJcD")).toBe(false);
    expect(isConfined(keys, C, undefined)).toBe(false);
  });
});
