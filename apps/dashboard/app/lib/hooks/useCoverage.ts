"use client";
// Reads a certificate's economics via /api/coverage — the premium the
// PremiumVault charges for it and the spend the PaymentRouter has metered
// against it. Sibling of useCert, same shape and same polling contract, kept
// separate because it reaches two more contracts and /dashboard's cert lookup
// should not wait on them.
import { useCallback, useEffect, useState } from "react";
import type { CoverageView } from "@/app/lib/coverage-view";

export function useCoverage(address: string | null, opts?: { pollMs?: number }) {
  const [coverage, setCoverage] = useState<CoverageView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refetch = useCallback(async () => {
    if (!address) return;
    setLoading(true);
    try {
      const res = await fetch(`/api/coverage?agent=${encodeURIComponent(address)}`, {
        cache: "no-store",
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data?.error ?? "coverage read failed");
      setCoverage(data as CoverageView);
      setError(null);
    } catch (e) {
      // Keep the last good reading on screen and surface the error alongside it.
      // Blanking the panel on a transient RPC failure would look like "no
      // coverage", which is a claim about the chain rather than about the fetch.
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [address]);

  useEffect(() => {
    // Drop the previous agent's reading before fetching the new one. Polling does
    // not re-run this effect, so the only thing it clears is a view that belongs
    // to a different address — and leaving that on screen for a round trip would
    // attribute one agent's coverage and spend to another.
    setCoverage(null);
    setError(null);
    if (!address) return;
    refetch();
    if (opts?.pollMs) {
      const id = setInterval(refetch, opts.pollMs);
      return () => clearInterval(id);
    }
  }, [address, opts?.pollMs, refetch]);

  return { coverage, loading, error, refetch };
}
