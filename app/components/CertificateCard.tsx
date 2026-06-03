// Shared Bound Certificate card — the single source of cert rendering, reused by
// /dashboard, the /chat side-panel, and /control. Takes a CertView (the JSON-safe
// projection the API returns) and shows the numbers a counterparty reads before
// transacting: bound (worst-case), locked reserve, slashable auditor stake.
import type { CertView } from "@/app/lib/cert-view";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { StatusBadge } from "./StatusBadge";
import { AddressPill } from "./AddressPill";
import { UsdAmount } from "./UsdAmount";
import { cn } from "@/lib/utils";

function Field({
  label,
  children,
  hint,
}: {
  label: string;
  children: React.ReactNode;
  hint?: string;
}) {
  return (
    <div className="space-y-1">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="text-sm">{children}</div>
      {hint && <div className="text-[11px] text-muted-foreground">{hint}</div>}
    </div>
  );
}

export function CertificateCard({ cert, className }: { cert: CertView; className?: string }) {
  if (!cert.hasCert) {
    return (
      <Card className={className}>
        <CardHeader>
          <CardTitle>Bound Certificate</CardTitle>
          <CardDescription>No certificate is mapped to this agent yet.</CardDescription>
        </CardHeader>
        <CardContent>
          <Field label="Agent">
            <AddressPill address={cert.agent} showRole />
          </Field>
        </CardContent>
      </Card>
    );
  }

  const accent =
    cert.status === "Verified" && cert.valid
      ? "border-emerald-500/30"
      : cert.status === "Invalid"
        ? "border-red-500/30"
        : "border-amber-500/30";

  return (
    <Card className={cn(accent, className)}>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <CardTitle>Bound Certificate{cert.certId != null ? ` #${cert.certId}` : ""}</CardTitle>
          <StatusBadge status={cert.status} />
        </div>
        <CardDescription>
          <AddressPill address={cert.agent} showRole />
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-4">
        <div className="grid grid-cols-3 gap-4">
          <Field label="Bound" hint="worst-case loss">
            <UsdAmount value={cert.boundUsd} className="text-base font-semibold" />
          </Field>
          <Field label="Reserve" hint="locked, pre-funded">
            <UsdAmount value={cert.reserveUsd} className="text-base font-semibold" />
          </Field>
          <Field label="Auditor stake" hint="slashable">
            <UsdAmount value={cert.auditorStakeUsd} className="text-base font-semibold" />
          </Field>
        </div>

        <Separator />

        <div className="grid grid-cols-2 gap-4">
          <Field label="Auditor">
            {cert.auditor ? <AddressPill address={cert.auditor} showRole /> : <span className="text-muted-foreground">—</span>}
          </Field>
          <Field label="Expires">
            <span className="text-muted-foreground">
              {cert.expiresAtIso ? new Date(cert.expiresAtIso).toLocaleString() : "—"}
            </span>
          </Field>
        </div>
      </CardContent>

      <CardFooter className="text-xs text-muted-foreground">
        {cert.status === "Invalid" ? (
          <span>
            This attestation was challenged and invalidated on-chain — the auditor was slashed.
          </span>
        ) : (
          <span>
            Worst-case loss is bounded at <UsdAmount value={cert.boundUsd} muted />, pre-funded by a{" "}
            <UsdAmount value={cert.reserveUsd} muted /> locked reserve, with{" "}
            <UsdAmount value={cert.auditorStakeUsd} muted /> of auditor capital staked on it.
          </span>
        )}
      </CardFooter>
    </Card>
  );
}
