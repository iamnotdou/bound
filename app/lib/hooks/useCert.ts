"use client";
// Reads a Bound Certificate for an agent via /api/verify. Used by /dashboard
// (manual), and by /chat + /control (polled, so the panel flips Verified→Invalid
// the moment a slash settles). Pass pollMs to auto-refresh.
import { useCallback, useEffect, useState } from "react";
import type { CertView } from "../cert-view";

export function useCert(
  address: string | null,
  opts?: { pollMs?: number; enabled?: boolean },
) {
  const [cert, setCert] = useState<CertView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refetch = useCallback(async () => {
    if (!address) return;
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/verify", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ agent: address }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data?.error ?? "verify failed");
      setCert(data as CertView);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [address]);

  useEffect(() => {
    if (opts?.enabled === false || !address) return;
    refetch();
    if (opts?.pollMs) {
      const id = setInterval(refetch, opts.pollMs);
      return () => clearInterval(id);
    }
  }, [address, opts?.pollMs, opts?.enabled, refetch]);

  return { cert, loading, error, refetch, setCert };
}
