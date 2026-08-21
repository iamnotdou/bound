// Offline: the registry binding is mocked, so no RPC call is made.
import { describe, it, expect, vi, beforeEach } from "vitest";

const getCertCount = vi.fn();
const getCertificate = vi.fn();

vi.mock("../../../bindings/registry/src", () => ({
  Client: class {
    get_cert_count = getCertCount;
    get_certificate = getCertificate;
  },
}));

import { listCertificates, getCertificate as readCertificate } from "./discovery";

const AGENT = "GCPOBMCWPO5A24KJJJRD27T4TKITHQI5MYY2FCQRR3HUXUFT4LO473ZT";
const AUDITOR = "GCBVHBXWW7CIFKZSGHOZIRYUYIS6S455EW64QK4FXZZ7WVW6R2FZN523";

const FUTURE = BigInt(Math.floor(Date.now() / 1000) + 86_400);

function cert(over: Record<string, unknown> = {}) {
  return {
    agent: AGENT,
    operator: AGENT,
    auditor: AUDITOR,
    bound: 100_000_000_000n,
    reserve_amount: 40_000_000_000n,
    auditor_stake_snapshot: 50_000_000_000n,
    issued_at: 1n,
    expires_at: FUTURE,
    reserve_vault_contract: AGENT,
    auditor_staking_contract: AGENT,
    status: { tag: "Verified", values: undefined },
    ...over,
  };
}

function count(n: number) {
  getCertCount.mockResolvedValue({ result: BigInt(n) });
}

beforeEach(() => {
  getCertCount.mockReset();
  getCertificate.mockReset();
  getCertificate.mockImplementation(async () => ({ result: cert() }));
});

describe("listCertificates()", () => {
  it("returns newest first (highest cert id first)", async () => {
    count(3);
    const items = await listCertificates();
    expect(items.map((i) => i.certId)).toEqual([3, 2, 1]);
  });

  it("defaults to a limit of 50", async () => {
    count(120);
    const items = await listCertificates();
    expect(items).toHaveLength(50);
    expect(items[0].certId).toBe(120);
    expect(items[49].certId).toBe(71);
  });

  it("applies limit and offset", async () => {
    count(10);
    const items = await listCertificates({ limit: 3, offset: 2 });
    expect(items.map((i) => i.certId)).toEqual([8, 7, 6]);
  });

  it("stops at cert id 1 — ids are 1-based", async () => {
    count(2);
    const items = await listCertificates({ limit: 10, offset: 1 });
    expect(items.map((i) => i.certId)).toEqual([1]);
  });

  it("returns nothing for an empty registry, a zero limit, or an offset past the end", async () => {
    count(0);
    expect(await listCertificates()).toEqual([]);
    count(5);
    expect(await listCertificates({ limit: 0 })).toEqual([]);
    expect(await listCertificates({ offset: 99 })).toEqual([]);
  });

  it("skips certificates that fail to load instead of throwing", async () => {
    count(3);
    getCertificate.mockImplementation(async ({ cert_id }: { cert_id: bigint }) => {
      if (cert_id === 2n) throw new Error("certificate_not_found");
      return { result: cert() };
    });
    const items = await listCertificates();
    expect(items.map((i) => i.certId)).toEqual([3, 1]);
  });

  it("projects through toCertView — JSON-safe, no bigint survives", async () => {
    count(1);
    const [item] = await listCertificates();
    expect(item.agent).toBe(AGENT);
    expect(item.auditor).toBe(AUDITOR);
    expect(item.status).toBe("Verified");
    expect(item.valid).toBe(true);
    expect(item.boundUsd).toBe(`$${(10_000).toLocaleString()}`);
    expect(item.certId).toBe(1);
    expect(() => JSON.stringify(item)).not.toThrow();
  });

  it("marks an expired certificate invalid even when its status is Verified", async () => {
    count(1);
    getCertificate.mockResolvedValue({ result: cert({ expires_at: 1n }) });
    const [item] = await listCertificates();
    expect(item.status).toBe("Verified");
    expect(item.valid).toBe(false);
  });
});

describe("getCertificate", () => {
  it("reads one certificate by id and carries the id through", async () => {
    const got = await readCertificate(7);
    expect(getCertificate).toHaveBeenCalledWith({ cert_id: 7n });
    expect(got?.certId).toBe(7);
    expect(got?.agent).toBe(AGENT);
  });

  it("returns null for a missing certificate rather than throwing", async () => {
    getCertificate.mockRejectedValueOnce(new Error("certificate_not_found"));
    await expect(readCertificate(999)).resolves.toBeNull();
  });

  it("rejects non-positive and non-integer ids without touching the chain", async () => {
    for (const bad of [0, -1, 1.5, NaN]) {
      await expect(readCertificate(bad)).resolves.toBeNull();
    }
    expect(getCertificate).not.toHaveBeenCalled();
  });
});
