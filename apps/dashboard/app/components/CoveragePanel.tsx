// The coverage premium behind one certificate — what the operator pays for it,
// and what that payment earns the auditor who staked capital on it.
//
// Every figure here is read from the PremiumVault, not derived from what the
// certificate looks like it ought to have. The distinction the panel exists to
// keep sharp is the one an operator gets wrong: a *quote* is a price nobody has
// paid, and a certificate that has not been covered has earned its auditor
// exactly nothing. Those are rendered as different things, always.
import type { CoverageView } from "@/app/lib/coverage-view";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { AddressPill } from "./AddressPill";
import { UsdAmount } from "./UsdAmount";
import { Field } from "./Field";
import { Bar } from "./Bar";
import { cn } from "@/lib/utils";

function when(isoDate: string | null): string {
  return isoDate ? new Date(isoDate).toLocaleDateString() : "—";
}

function percent(fraction: number | null): string {
  return fraction === null ? "—" : `${(fraction * 100).toFixed(1)}%`;
}

function Shell({
  badge,
  badgeClass,
  children,
  footer,
  className,
}: {
  badge: string;
  badgeClass?: string;
  children: React.ReactNode;
  footer: React.ReactNode;
  className?: string;
}) {
  return (
    <Card className={className}>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <CardTitle>Coverage premium</CardTitle>
          <Badge variant="outline" className={cn("border-transparent", badgeClass)}>
            {badge}
          </Badge>
        </div>
        <CardDescription>
          An ongoing premium priced on the bound and the certificate&apos;s term. It accrues to the
          auditor as yield on the capital they staked, less the protocol&apos;s share.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">{children}</CardContent>
      <CardFooter className="text-xs text-muted-foreground">{footer}</CardFooter>
    </Card>
  );
}

export function CoveragePanel({ view, className }: { view: CoverageView; className?: string }) {
  const { premium, status } = view;
  const record = premium.record;

  if (premium.state === "no-cert") {
    return (
      <Shell
        badge="No certificate"
        badgeClass="bg-muted text-muted-foreground"
        className={className}
        footer={
          <span>
            The premium is priced from a certificate&apos;s bound and term, so there is nothing to
            price until one is published.
          </span>
        }
      >
        <p className="text-sm text-muted-foreground">
          No certificate is mapped to this agent, so no coverage exists and none can be bought.
        </p>
      </Shell>
    );
  }

  // Published, but the vault will not sell coverage on it. `pay_premium` requires
  // a verified certificate, and the reason is the whole economics of the thing:
  // the premium is yield on an auditor's staked capital, so there has to be an
  // auditor with capital allocated to this certificate before there is anywhere
  // for it to accrue.
  if (premium.state === "not-verified") {
    return (
      <Shell
        badge="Not for sale"
        badgeClass="bg-amber-500/15 text-amber-600 dark:text-amber-400"
        className={className}
        footer={
          <span>
            The price is already fixed — it is computed from the bound and the term recorded when
            the certificate was published, and both are immutable.
          </span>
        }
      >
        <div className="grid grid-cols-2 gap-4">
          <Field label="Would cost" hint="if an auditor attests it">
            <UsdAmount value={premium.quotedUsd ?? "—"} className="text-base font-semibold" />
          </Field>
          <Field label="Premium paid" hint="nothing has been paid">
            <span className="text-base font-semibold text-muted-foreground">None</span>
          </Field>
        </div>
        <p className="text-sm text-muted-foreground">
          {status === "Invalid"
            ? "This certificate was invalidated on-chain. The vault will not sell coverage on it, and no auditor stands behind it."
            : "No auditor has attested this certificate yet, so the vault refuses payment. The premium is yield on an auditor's staked capital — until one has attested there is nobody for it to accrue to."}{" "}
          No premium has been paid and no yield has accrued.
        </p>
      </Shell>
    );
  }

  // Verified and priced, but nobody has bought it. Rendered as a quote, and the
  // absence of yield is stated rather than left to be inferred from a zero.
  if (premium.state === "quoted") {
    return (
      <Shell
        badge="Quoted, not bought"
        badgeClass="bg-amber-500/15 text-amber-600 dark:text-amber-400"
        className={className}
        footer={
          <span>
            Priced as bound × rate × term, annualised. The term is the certificate&apos;s whole life
            — issue to expiry — not the time left on it, so the price is fixed at publish and
            waiting does not make coverage cheaper.
          </span>
        }
      >
        <div className="grid grid-cols-2 gap-4">
          <Field label="Quote" hint="what the operator would pay, once">
            <UsdAmount value={premium.quotedUsd ?? "—"} className="text-base font-semibold" />
          </Field>
          <Field label="Term ends" hint="the certificate's expiry">
            <span className="text-muted-foreground">{when(premium.termEndsIso)}</span>
          </Field>
        </div>
        <p className="text-sm text-muted-foreground">
          This is a quote. No premium has been paid for this certificate, so its auditor has accrued
          no yield on it.
        </p>
      </Shell>
    );
  }

  if (!record) return null;

  const closed = premium.state === "closed";

  return (
    <Shell
      badge={closed ? "Coverage closed" : "Coverage active"}
      badgeClass={
        closed
          ? "bg-red-500/15 text-red-600 dark:text-red-400"
          : "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400"
      }
      className={className}
      footer={
        closed ? (
          <span>
            Coverage closes when a certificate is settled against, and the pot was frozen at the
            figure above. What was in it beyond that left in the settlement — to the protocol
            treasury, and on a slash to the harmed party first, capped by the harm actually proven.
            Yield the auditor had already withdrawn is theirs and is not clawed back.
          </span>
        ) : (
          <span>
            Yield accrues straight-line across the term, starting at the certificate&apos;s issue
            date rather than at the payment. The auditor can withdraw at any point; a slash forfeits
            only what is still unclaimed, and it is their allocated stake — not this pot — that a
            slash really takes.
          </span>
        )
      }
    >
      <div className="grid grid-cols-3 gap-4">
        <Field label="Premium paid" hint="once, by the operator">
          <UsdAmount value={record.premiumUsd} className="text-base font-semibold" />
        </Field>
        <Field label="Protocol fee" hint="to the treasury, on payment">
          <UsdAmount value={record.protocolFeeUsd} className="text-base font-semibold" />
        </Field>
        <Field label="Auditor's pot" hint="the most they can draw">
          <UsdAmount value={record.yieldPotUsd} className="text-base font-semibold" />
        </Field>
      </div>

      <Separator />

      <div className="space-y-2">
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-xs font-medium text-muted-foreground">
            Yield accrued to the auditor
          </span>
          <span className="text-[11px] text-muted-foreground">
            {/* Closing rewrites the pot down to the frozen figure, so a closed
                coverage always reads 100% — of a pot that is no longer the one
                the operator paid for. Say which pot, or the bar reads as "the
                auditor earned the whole premium". */}
            {percent(record.accruedFraction)} of the pot{closed ? " as frozen" : ""}
          </span>
        </div>
        <Bar value={record.accruedFraction} tone={closed ? "neutral" : "good"} />
        <div className="grid grid-cols-3 gap-4 pt-1">
          <Field label="Accrued" hint={closed ? "frozen at close" : "earned so far"}>
            <UsdAmount value={record.accruedUsd} className="text-base font-semibold" />
          </Field>
          <Field label="Withdrawn" hint="already claimed">
            <UsdAmount value={record.claimedUsd} className="text-base font-semibold" />
          </Field>
          <Field label="Claimable now" hint="accrued minus withdrawn">
            <UsdAmount value={record.claimableUsd} className="text-base font-semibold" />
          </Field>
        </div>
      </div>

      <Separator />

      <div className="grid grid-cols-2 gap-4">
        <Field label="Accrues to" hint="the auditor named at payment">
          <AddressPill address={record.auditor} showRole />
        </Field>
        <Field
          label={closed ? "Term (closed early)" : "Term"}
          hint={
            closed
              ? `accrual stopped ${when(record.closedAtIso)}`
              : `${record.termDays.toFixed(0)} days`
          }
        >
          <span className="text-muted-foreground">
            {when(record.startIso)} → {when(record.endIso)}
          </span>
        </Field>
      </div>
    </Shell>
  );
}
