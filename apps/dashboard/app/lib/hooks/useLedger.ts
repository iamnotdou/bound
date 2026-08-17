"use client";
// Polls /api/ledger — the proof board's data source: every actor's USDC balance
// plus the contract state that matters (reserve held vs claimed, auditor stake,
// cert). Each actor action refetches, so every click produces a visible state
// diff — the diff is the proof.
import { useCallback, useEffect, useState } from "react";
import type { CertView } from "../cert-view";

export interface LedgerAccount {
  role: string;
  address: string;
  usdc: string;
  /** The USDC issuer (operator) — its balance is unbounded, so render "∞". */
  isIssuer?: boolean;
}

export interface Ledger {
  accounts: LedgerAccount[];
  contracts: {
    reserveHeldUsd: string;
    reserveClaimedUsd: string;
    auditorStakeUsd: string;
  };
  cert: CertView;
}

export function useLedger(pollMs = 5000) {
  const [ledger, setLedger] = useState<Ledger | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const refetch = useCallback(async () => {
    try {
      const res = await fetch("/api/ledger", { cache: "no-store" });
      const data = await res.json();
      if (!res.ok) throw new Error(data?.error ?? "ledger read failed");
      setLedger(data as Ledger);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refetch();
    const id = setInterval(refetch, pollMs);
    return () => clearInterval(id);
  }, [refetch, pollMs]);

  return { ledger, error, loading, refetch };
}
