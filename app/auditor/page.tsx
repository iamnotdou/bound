"use client";
// /auditor — the vouch surface. An independent auditor reviews the attestation
// they're being asked to stand behind (operator, agent, the claimed bound +
// reserve, and the stake that locks the instant they sign) and signs it. In the
// demo the auditor keypair signs server-side, so "Sign & Publish" republishes a
// fresh PENDING cert and immediately attests it → VERIFIED. That doubles as the
// demo reset: it undoes a prior slash so the climax can be run again.
//
// All signing is server-side (POST /api/auditor); no secret key reaches here.
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { CertificateCard } from "@/app/components/CertificateCard";
import { AddressPill } from "@/app/components/AddressPill";
import { UsdAmount } from "@/app/components/UsdAmount";
import type { CertView } from "@/app/lib/cert-view";

interface Pending {
  operator: string;
  agent: string;
  auditor: string;
  boundUsd: string;
  reserveClaimedUsd: string;
  auditorStakeUsd: string;
  registered: boolean;
  willStakeOnSign: boolean;
  expiryDays: number;
}

function Field({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div className="space-y-1">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="text-sm">{children}</div>
      {hint && <div className="text-[11px] text-muted-foreground">{hint}</div>}
    </div>
  );
}

export default function AuditorPage() {
  const [pending, setPending] = useState<Pending | null>(null);
  const [current, setCurrent] = useState<CertView | null>(null);
  const [loading, setLoading] = useState(true);
  const [signing, setSigning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch("/api/auditor");
      const data = await res.json();
      if (!res.ok) throw new Error(data?.error ?? "failed to read attestation request");
      setPending(data.pending as Pending);
      setCurrent(data.current as CertView);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function signAndPublish() {
    setSigning(true);
    try {
      const res = await fetch("/api/auditor", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ action: "sign-publish" }),
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data?.error ?? "sign & publish failed");
      setCurrent(data.cert as CertView);
      toast.success(`Signed & published cert #${data.certId} → Verified`);
      // refresh the pending panel (registration state may have changed)
      void load();
    } catch (e) {
      toast.error((e as Error).message);
    } finally {
      setSigning(false);
    }
  }

  return (
    <main className="mx-auto max-w-5xl px-4 py-8">
      <div className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Auditor</h1>
        <p className="text-sm text-muted-foreground">
          Review the attestation you&apos;re asked to vouch for, then sign it with your own
          slashable capital. The moment you attest, your stake locks to the certificate — if the
          claim is false, anyone can prove it on-chain and you lose it.
        </p>
      </div>

      <div className="mt-6 grid gap-6 lg:grid-cols-2">
        {/* ── attestation request ─────────────────────────────────────── */}
        <Card>
          <CardHeader>
            <CardTitle>Attestation request</CardTitle>
            <CardDescription>
              What the operator is asking you to stand behind. The reserve is the{" "}
              <em>claimed</em> coverage you sign off on.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            {loading && !pending ? (
              <Skeleton className="h-48 w-full rounded-lg" />
            ) : error ? (
              <p className="text-sm text-red-600 dark:text-red-400">{error}</p>
            ) : pending ? (
              <>
                <div className="grid grid-cols-2 gap-4">
                  <Field label="Operator">
                    <AddressPill address={pending.operator} showRole />
                  </Field>
                  <Field label="Agent">
                    <AddressPill address={pending.agent} showRole />
                  </Field>
                  <Field label="Auditor (you)">
                    <AddressPill address={pending.auditor} showRole />
                  </Field>
                  <Field label="Expiry">
                    <span className="text-muted-foreground">{pending.expiryDays} days</span>
                  </Field>
                </div>

                <Separator />

                <div className="grid grid-cols-3 gap-4">
                  <Field label="Bound" hint="attested worst case">
                    <UsdAmount value={pending.boundUsd} className="text-base font-semibold" />
                  </Field>
                  <Field label="Reserve" hint="claimed, pre-funded">
                    <UsdAmount value={pending.reserveClaimedUsd} className="text-base font-semibold" />
                  </Field>
                  <Field label="Your stake" hint="slashable">
                    <UsdAmount value={pending.auditorStakeUsd} className="text-base font-semibold" />
                  </Field>
                </div>

                <div className="rounded-lg border border-amber-500/30 bg-amber-500/[0.04] px-3 py-2 text-xs text-muted-foreground">
                  {pending.willStakeOnSign ? (
                    <>
                      Signing will stake{" "}
                      <UsdAmount value={pending.auditorStakeUsd} muted /> of your own capital and lock
                      it to this certificate until expiry.
                    </>
                  ) : (
                    <>
                      Your <UsdAmount value={pending.auditorStakeUsd} muted /> stake is already
                      registered and will lock to this certificate the instant you attest.
                    </>
                  )}
                </div>

                <Button onClick={signAndPublish} disabled={signing} className="w-full">
                  {signing ? "Signing…" : "Sign & Publish"}
                </Button>
                <p className="text-[11px] text-muted-foreground">
                  Demo: the auditor keypair signs server-side. This republishes and re-attests a
                  fresh certificate — also the reset that restores a Verified cert after a slash.
                </p>
              </>
            ) : null}
          </CardContent>
        </Card>

        {/* ── current on-chain certificate ────────────────────────────── */}
        <div className="space-y-2">
          <h2 className="text-sm font-medium">Current certificate</h2>
          {loading && !current ? (
            <Skeleton className="h-64 w-full rounded-xl" />
          ) : current ? (
            <CertificateCard cert={current} />
          ) : (
            <p className="text-sm text-muted-foreground">No certificate read yet.</p>
          )}
          <p className="text-xs text-muted-foreground">
            The live cert this agent currently carries. After you sign, it flips to{" "}
            <span className="font-medium text-emerald-600 dark:text-emerald-400">Verified</span>.
          </p>
        </div>
      </div>
    </main>
  );
}
