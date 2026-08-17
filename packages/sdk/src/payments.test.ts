import { describe, it, expect, vi, afterEach } from "vitest";
import { Keypair } from "@stellar/stellar-sdk";
import { agentFetch } from "./payments";
import type { BoundClient } from "./bound-client";

const RECIPIENT = "GDWIIDO6AYJ7KMQBZALWUEWA2TRUQUIV7R673LL536TSGCEBQ4BDYZSZ";
const URL = "https://example.test/paid";

/**
 * A BoundClient stand-in that records payments instead of making them.
 *
 * Takes an options object rather than a positional argument with a default,
 * because a default parameter also fires when `undefined` is passed explicitly
 * -- which is exactly the "chain returned no hash" case one test needs.
 */
function stubBound(opts: { txHash?: string } = {}) {
  const txHash = "txHash" in opts ? opts.txHash : "abc123";
  const calls: { recipient: string; amount: bigint }[] = [];
  const bound = {
    executePayment: vi.fn(async (_signer: Keypair, recipient: string, amount: bigint) => {
      calls.push({ recipient, amount });
      return txHash;
    }),
  } as unknown as BoundClient;
  return { bound, calls };
}

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("agentFetch()", () => {
  it("passes a non-402 response straight through and pays nothing", async () => {
    const fetchMock = vi.fn(async () => json(200, { ok: true }));
    vi.stubGlobal("fetch", fetchMock);
    const { bound, calls } = stubBound();

    const result = await agentFetch(URL, Keypair.random(), bound);

    expect(result.response.status).toBe(200);
    expect(result.paid).toBeUndefined();
    expect(calls).toHaveLength(0);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("does not treat other error codes as payment demands", async () => {
    const fetchMock = vi.fn(async () => json(403, { error: "forbidden" }));
    vi.stubGlobal("fetch", fetchMock);
    const { bound, calls } = stubBound();

    const result = await agentFetch(URL, Keypair.random(), bound);

    expect(result.response.status).toBe(403);
    expect(calls).toHaveLength(0);
  });

  describe("on 402 Payment Required", () => {
    it("pays the demanded price and retries once", async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(json(402, { amount: 25, recipient: RECIPIENT, asset: "USDC" }))
        .mockResolvedValueOnce(json(200, { ok: true }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound, calls } = stubBound({ txHash: "deadbeef" });

      const result = await agentFetch(URL, Keypair.random(), bound);

      expect(fetchMock).toHaveBeenCalledTimes(2);
      expect(result.response.status).toBe(200);
      expect(calls).toHaveLength(1);
      // The server names the price in dollars; it must reach the chain as
      // stroops. $25 -> 25 * 10^7.
      expect(calls[0]).toEqual({ recipient: RECIPIENT, amount: 250_000_000n });
      expect(result.paid).toEqual({ amount: 25, recipient: RECIPIENT, txHash: "deadbeef" });
    });

    it("sends the tx hash as proof on the retry", async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(json(402, { amount: 1, recipient: RECIPIENT }))
        .mockResolvedValueOnce(json(200, { ok: true }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound } = stubBound({ txHash: "proofhash" });

      await agentFetch(URL, Keypair.random(), bound);

      const retryInit = fetchMock.mock.calls[1][1] as RequestInit;
      expect((retryInit.headers as Record<string, string>)["X-Payment"]).toBe("proofhash");
    });

    it("preserves caller headers and init on the retry", async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(json(402, { amount: 1, recipient: RECIPIENT }))
        .mockResolvedValueOnce(json(200, { ok: true }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound } = stubBound();

      await agentFetch(URL, Keypair.random(), bound, {
        method: "POST",
        headers: { "X-Trace": "t-1" },
      });

      const retryInit = fetchMock.mock.calls[1][1] as RequestInit;
      const headers = retryInit.headers as Record<string, string>;
      expect(retryInit.method).toBe("POST");
      expect(headers["X-Trace"]).toBe("t-1");
      expect(headers["X-Payment"]).toBeDefined();
    });

    it("sends an empty proof header when no tx hash comes back", async () => {
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(json(402, { amount: 1, recipient: RECIPIENT }))
        .mockResolvedValueOnce(json(200, { ok: true }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound } = stubBound({ txHash: undefined });

      const result = await agentFetch(URL, Keypair.random(), bound);

      const retryInit = fetchMock.mock.calls[1][1] as RequestInit;
      expect((retryInit.headers as Record<string, string>)["X-Payment"]).toBe("");
      expect(result.paid?.txHash).toBeUndefined();
    });

    it("rejects a demand with no amount, without paying", async () => {
      const fetchMock = vi.fn(async () => json(402, { recipient: RECIPIENT }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound, calls } = stubBound();

      await expect(agentFetch(URL, Keypair.random(), bound)).rejects.toThrow(/malformed 402/);
      expect(calls).toHaveLength(0);
      expect(fetchMock).toHaveBeenCalledTimes(1);
    });

    it("rejects a demand with no recipient, without paying", async () => {
      const fetchMock = vi.fn(async () => json(402, { amount: 10 }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound, calls } = stubBound();

      await expect(agentFetch(URL, Keypair.random(), bound)).rejects.toThrow(/malformed 402/);
      expect(calls).toHaveLength(0);
    });

    it("rejects a zero-amount demand, which would otherwise be a free retry", async () => {
      const fetchMock = vi.fn(async () => json(402, { amount: 0, recipient: RECIPIENT }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound, calls } = stubBound();

      await expect(agentFetch(URL, Keypair.random(), bound)).rejects.toThrow(/malformed 402/);
      expect(calls).toHaveLength(0);
    });

    it("pays the price the server names, however large", async () => {
      // The point of the bound: the agent does not second-guess the demand,
      // the certificate caps the counterparty's exposure instead.
      const fetchMock = vi
        .fn()
        .mockResolvedValueOnce(json(402, { amount: 10_000_000, recipient: RECIPIENT }))
        .mockResolvedValueOnce(json(200, { ok: true }));
      vi.stubGlobal("fetch", fetchMock);
      const { bound, calls } = stubBound();

      await agentFetch(URL, Keypair.random(), bound);

      expect(calls[0].amount).toBe(100_000_000_000_000n);
    });
  });
});
