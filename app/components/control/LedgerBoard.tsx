"use client";
// The proof surface: 5 named accounts + the contract state that matters. Values
// flash on change so each actor action produces a visible diff. The reserve goes
// red when held < claimed — the exact condition the challenge proves on-chain.
import { useEffect, useRef, useState } from "react";
import type { Ledger } from "@/app/lib/hooks/useLedger";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { AddressPill } from "../AddressPill";
import { UsdAmount } from "../UsdAmount";
import { StatusBadge } from "../StatusBadge";
import { cn } from "@/lib/utils";

const toNum = (s: string) => Number(s.replace(/[^0-9.]/g, "")) || 0;

export function LedgerBoard({ ledger, loading }: { ledger: Ledger | null; loading: boolean }) {
  const prev = useRef<Record<string, string>>({});
  const [flash, setFlash] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (!ledger) return;
    const next: Record<string, string> = {};
    const changed: Record<string, boolean> = {};
    // skip the issuer — its unbounded balance drifts on fees and shouldn't flash
    for (const a of ledger.accounts) if (!a.isIssuer) next[a.role] = a.usdc;
    next.reserve = ledger.contracts.reserveHeldUsd;
    next.stake = ledger.contracts.auditorStakeUsd;
    for (const [k, v] of Object.entries(next)) {
      if (prev.current[k] !== undefined && prev.current[k] !== v) changed[k] = true;
    }
    prev.current = next;
    if (Object.keys(changed).length) {
      setFlash(changed);
      const t = setTimeout(() => setFlash({}), 1400);
      return () => clearTimeout(t);
    }
  }, [ledger]);

  if (!ledger) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Ledger</CardTitle>
        </CardHeader>
        <CardContent className="text-sm text-muted-foreground">
          {loading ? "Reading chain…" : "—"}
        </CardContent>
      </Card>
    );
  }

  const short = toNum(ledger.contracts.reserveHeldUsd) < toNum(ledger.contracts.reserveClaimedUsd);

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between">
        <CardTitle>Ledger</CardTitle>
        <StatusBadge status={ledger.cert.status} />
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="space-y-0.5">
          {ledger.accounts.map((a) => (
            <div
              key={a.role}
              className={cn(
                "flex items-center justify-between rounded-md px-2 py-1.5 transition-colors duration-700",
                flash[a.role] && "bg-emerald-500/15",
              )}
            >
              <div className="flex items-center gap-2">
                <span className="w-24 text-sm font-medium">{a.role}</span>
                <AddressPill address={a.address} />
              </div>
              {a.isIssuer ? (
                <span className="font-mono text-sm text-muted-foreground" title="USDC issuer — balance is unbounded">
                  ∞ <span className="text-[11px]">(issuer)</span>
                </span>
              ) : (
                <UsdAmount value={a.usdc} className="text-sm" />
              )}
            </div>
          ))}
        </div>

        <Separator />

        <div className="grid grid-cols-3 gap-3">
          <div className={cn("rounded-md p-2 transition-colors duration-700", flash.reserve && "bg-emerald-500/15")}>
            <div className="text-xs text-muted-foreground">Reserve held</div>
            <UsdAmount
              value={ledger.contracts.reserveHeldUsd}
              className={cn("text-base font-semibold", short && "text-red-600 dark:text-red-400")}
            />
            <div className="text-[11px] text-muted-foreground">
              claims {ledger.contracts.reserveClaimedUsd}
              {short && <span className="text-red-600 dark:text-red-400"> · SHORT</span>}
            </div>
          </div>
          <div className={cn("rounded-md p-2 transition-colors duration-700", flash.stake && "bg-emerald-500/15")}>
            <div className="text-xs text-muted-foreground">Auditor stake</div>
            <UsdAmount value={ledger.contracts.auditorStakeUsd} className="text-base font-semibold" />
            <div className="text-[11px] text-muted-foreground">slashable</div>
          </div>
          <div className="rounded-md p-2">
            <div className="text-xs text-muted-foreground">Certificate</div>
            <div className="text-base font-semibold">
              {ledger.cert.certId != null ? `#${ledger.cert.certId}` : "—"}
            </div>
            <div className="text-[11px] text-muted-foreground">{ledger.cert.boundUsd} bound</div>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
