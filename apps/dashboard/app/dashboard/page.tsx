"use client";
// /dashboard — read a Bound Certificate before you transact. The standalone
// "read before you transact" surface: enter an agent address → /api/verify reads
// the live testnet cert → CertificateCard. No wallet needed.
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { CertificateCard } from "@/app/components/CertificateCard";
import { CoveragePanel } from "@/app/components/CoveragePanel";
import { SpendMeter } from "@/app/components/SpendMeter";
import { useCoverage } from "@/app/lib/hooks/useCoverage";
import { roles } from "@/app/lib/ui-config";
import type { CertView } from "@bound/sdk";

const G_ADDRESS = /^G[A-Z2-7]{55}$/;

export default function DashboardPage() {
  const [address, setAddress] = useState("");
  const [cert, setCert] = useState<CertView | null>(null);
  const [loading, setLoading] = useState(false);
  const [inlineError, setInlineError] = useState<string | null>(null);
  // The economics ride behind the certificate rather than inside it: /api/verify
  // answers "is this good?" in one round trip, and the premium and the meter —
  // two more contracts — load after it rather than holding it up.
  const { coverage, loading: coverageLoading } = useCoverage(cert?.agent ?? null);

  async function verify(addr: string) {
    const trimmed = addr.trim();
    if (!G_ADDRESS.test(trimmed)) {
      setInlineError("Enter a valid Stellar address (G…, 56 chars).");
      return;
    }
    setInlineError(null);
    setLoading(true);
    try {
      const res = await fetch("/api/verify", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ agent: trimmed }),
      });
      const data = await res.json();
      if (res.status === 400) {
        setInlineError(data?.error ?? "Invalid address.");
        return;
      }
      if (!res.ok) {
        toast.error(data?.error ?? "On-chain read failed.");
        return;
      }
      setCert(data as CertView);
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  function useDemoAgent() {
    setAddress(roles.agent.address);
    void verify(roles.agent.address);
  }

  return (
    <main className="mx-auto max-w-2xl px-4 py-10">
      <div className="space-y-2">
        <h1 className="text-2xl font-semibold tracking-tight">Verify a certificate</h1>
        <p className="text-sm text-muted-foreground">
          Read an agent&apos;s Bound Certificate before you transact — the bound, the locked
          reserve, the auditor&apos;s slashable stake, and whether it&apos;s still valid. Then the
          economics behind it: whether coverage was ever paid for, and what the agent has actually
          routed.
        </p>
      </div>

      <form
        className="mt-6 flex flex-col gap-3 sm:flex-row"
        onSubmit={(e) => {
          e.preventDefault();
          void verify(address);
        }}
      >
        <Input
          value={address}
          onChange={(e) => setAddress(e.target.value)}
          placeholder="Agent address (G…)"
          className="font-mono"
          aria-invalid={!!inlineError}
        />
        <div className="flex gap-2">
          <Button type="submit" disabled={loading}>
            {loading ? "Reading…" : "Verify"}
          </Button>
          <Button type="button" variant="outline" onClick={useDemoAgent} disabled={loading}>
            Use demo agent
          </Button>
        </div>
      </form>
      {inlineError && <p className="mt-2 text-sm text-red-600 dark:text-red-400">{inlineError}</p>}

      <div className="mt-8">
        {loading ? (
          <Skeleton className="h-64 w-full rounded-xl" />
        ) : cert ? (
          <CertificateCard cert={cert} />
        ) : (
          <p className="text-sm text-muted-foreground">
            Enter an address or use the demo agent to read a live testnet certificate.
          </p>
        )}
      </div>

      {cert && (
        <div className="mt-6 space-y-6">
          {coverage ? (
            <>
              <CoveragePanel view={coverage} />
              <SpendMeter view={coverage} />
            </>
          ) : coverageLoading ? (
            <>
              <Skeleton className="h-64 w-full rounded-xl" />
              <Skeleton className="h-64 w-full rounded-xl" />
            </>
          ) : null}
        </div>
      )}
    </main>
  );
}
