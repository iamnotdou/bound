"use client";
// Client orchestration for the connected-wallet path: build the unsigned XDR
// server-side, sign it in the wallet, submit it. The challenger flow also
// triggers the permissionless `resolve` afterward. Pages call run()/fund() and
// get back a tx hash — all the round-trips are hidden here.
import { useCallback } from "react";
import { useWallet } from "./useWallet";
import type { WalletAction, BuildParams } from "../tx-build";

async function post<T = unknown>(url: string, body: unknown): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data?.error ?? `${url} failed (${res.status})`);
  return data as T;
}

export function useWalletActions() {
  const { address, signXdr } = useWallet();

  /** Build → sign → submit a wallet action. For "challenge", also resolve it. */
  const run = useCallback(
    async (action: WalletAction, params: BuildParams = {}) => {
      if (!address) throw new Error("Connect a wallet first.");
      const { xdr } = await post<{ xdr: string }>("/api/tx/build", { action, address, params });
      const signed = await signXdr(xdr);
      const { hash, result } = await post<{ hash: string; result: unknown }>("/api/tx/submit", {
        xdr: signed,
      });

      if (action === "challenge" && typeof result === "number") {
        await post("/api/challenger/resolve", { challengeId: result });
      }
      return { hash, result };
    },
    [address, signXdr],
  );

  /** XLM (friendbot) → wallet-signed USDC trustline → operator mints test USDC. */
  const fund = useCallback(
    async (amountUsd?: number) => {
      if (!address) throw new Error("Connect a wallet first.");
      await post("/api/fund", { address, step: "xlm" });
      const { xdr } = await post<{ xdr: string }>("/api/fund", { address, step: "trustline" });
      const signed = await signXdr(xdr);
      await post("/api/tx/submit", { xdr: signed });
      return post<{ balanceUsd: string }>("/api/fund", { address, step: "mint", amountUsd });
    },
    [address, signXdr],
  );

  return { address, run, fund };
}
