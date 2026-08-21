// The PaymentRouter's spend meter for one certificate.
//
// Two things this panel must never say, because the contract does not say them:
//
//  1. That routed spend is loss. It is cumulative *gross flow*. A single dollar
//     shuttled between two addresses one operator controls drives the counter
//     past any bound for the price of gas, and nothing leaves that operator's
//     control. `spent > bound` is unforgeable evidence that a covenant about
//     conduct was broken; it is not an amount anybody owes, and the protocol
//     sizes no payout from it.
//  2. That an unenrolled agent is being watched. Only a payment from an enrolled
//     address moves the counter, so an unenrolled agent's payments leave no
//     evidence at all — and a meter reading $0 beside an unenrolled agent means
//     "the router saw nothing", never "the agent spent nothing".
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
import { UsdAmount } from "./UsdAmount";
import { Field } from "./Field";
import { Bar } from "./Bar";
import { cn } from "@/lib/utils";

function percent(fraction: number | null): string {
  return fraction === null ? "—" : `${(fraction * 100).toFixed(1)}%`;
}

function Notice({
  tone,
  children,
}: {
  tone: "muted" | "warn" | "danger";
  children: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        "rounded-md border p-3 text-xs leading-relaxed",
        tone === "muted" && "border-dashed text-muted-foreground",
        tone === "warn" && "border-amber-500/30 bg-amber-500/5 text-amber-700 dark:text-amber-400",
        tone === "danger" && "border-red-500/30 bg-red-500/5 text-red-700 dark:text-red-400",
      )}
    >
      {children}
    </div>
  );
}

export function SpendMeter({ view, className }: { view: CoverageView; className?: string }) {
  const { meter, certId } = view;

  const badge = !meter.agentEnrolled
    ? { text: "Unmetered", cls: "bg-muted text-muted-foreground" }
    : meter.halted
      ? { text: "Halted", cls: "bg-red-500/15 text-red-600 dark:text-red-400" }
      : meter.meteredHere
        ? { text: "Metered", cls: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400" }
        : { text: "Metered elsewhere", cls: "bg-amber-500/15 text-amber-600 dark:text-amber-400" };

  return (
    <Card className={className}>
      <CardHeader>
        <div className="flex items-center justify-between gap-2">
          <CardTitle>Spend meter</CardTitle>
          <Badge variant="outline" className={cn("border-transparent", badge.cls)}>
            {badge.text}
          </Badge>
        </div>
        <CardDescription>
          Payments made through the PaymentRouter are counted against the certificate the paying
          agent is enrolled under. Payments made any other way are not.
        </CardDescription>
      </CardHeader>

      <CardContent className="space-y-4">
        {!meter.agentEnrolled && (
          <Notice tone="muted">
            <strong className="font-medium text-foreground">
              This agent is not enrolled in the router.
            </strong>{" "}
            Its payments settle as ordinary USDC transfers: they never reach the meter, they add
            nothing to any certificate&apos;s counter, and they leave no on-chain record that a
            challenge could read. Nothing below is a statement about what this agent has spent.
          </Notice>
        )}

        {meter.agentEnrolled && !meter.meteredHere && (
          <Notice tone="warn">
            <strong className="font-medium">
              This agent is metered against certificate #{meter.agentCertId}
              {certId !== null ? `, not #${certId}` : ""}.
            </strong>{" "}
            An enrollment is permanent by design — an operator who republishes for the same agent
            cannot move the binding onto the newer certificate, because being able to walk an agent
            off a climbing counter would make the counter worthless as evidence. Its payments are
            still recorded against #{meter.agentCertId}.
          </Notice>
        )}

        {meter.halted && (
          <Notice tone="danger">
            <strong className="font-medium">Routing is halted for this certificate.</strong> The
            operator&apos;s kill switch is on, so no enrolled agent can transfer, withdraw or burn
            through the router. It freezes the float rather than rescuing it, and it says nothing
            about whether the certificate is valid.
          </Notice>
        )}

        {certId === null ? (
          <p className="text-sm text-muted-foreground">
            No certificate is mapped to this agent, so there is no counter to read.
          </p>
        ) : (
          <>
            <div className="space-y-2">
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-xs font-medium text-muted-foreground">
                  {meter.meteredHere ? "Routed spend" : `Routed spend on #${certId}`}
                </span>
                <span className="text-[11px] text-muted-foreground">
                  {percent(meter.spentFraction)} of the bound
                </span>
              </div>
              <Bar
                value={meter.spentFraction}
                tone={
                  meter.overBound
                    ? "over"
                    : (meter.spentFraction ?? 0) > 0.8
                      ? "warn"
                      : meter.spentFraction
                        ? "good"
                        : "neutral"
                }
              />
              <div className="grid grid-cols-2 gap-4 pt-1">
                <Field
                  label="Cumulative routed spend"
                  hint="gross flow through the meter, all time"
                >
                  <UsdAmount
                    value={meter.spentUsd}
                    className={cn(
                      "text-base font-semibold",
                      meter.overBound && "text-red-600 dark:text-red-400",
                    )}
                  />
                </Field>
                <Field label="Certified bound" hint="what the certificate promised">
                  <UsdAmount value={meter.boundUsd} className="text-base font-semibold" />
                </Field>
              </div>
            </div>

            {meter.overBound && (
              <Notice tone="danger">
                <strong className="font-medium">
                  Routed spend has passed the certified bound.
                </strong>{" "}
                That is the on-chain record a BoundExceeded challenge is proven from — the
                certificate broke a promise it made about its own conduct. It is not a loss, not an
                amount owed, and not evidence that anybody was harmed; a payout has to be sized by
                harm proven against a party outside the operator&apos;s control.
              </Notice>
            )}

            <Separator />

            <div className="space-y-2">
              <div className="flex items-baseline justify-between gap-2">
                <span className="text-xs font-medium text-muted-foreground">Float held</span>
                <span className="text-[11px] text-muted-foreground">
                  {meter.floatCapUsd ? `${percent(meter.floatFraction)} of the cap` : "no cap read"}
                </span>
              </div>
              <Bar
                value={meter.floatFraction}
                tone={
                  meter.overFloatCap
                    ? "over"
                    : (meter.floatFraction ?? 0) > 0.8
                      ? "warn"
                      : "neutral"
                }
              />
              <div className="grid grid-cols-3 gap-4 pt-1">
                <Field label="Float" hint="held by the router for this cert">
                  <UsdAmount value={meter.floatUsd} className="text-base font-semibold" />
                </Field>
                <Field
                  label="Float cap"
                  hint={meter.floatCapUsd ? "deposit ceiling" : "none reported by the router"}
                >
                  {meter.floatCapUsd ? (
                    <UsdAmount value={meter.floatCapUsd} className="text-base font-semibold" />
                  ) : (
                    <span className="text-base font-semibold text-muted-foreground">—</span>
                  )}
                </Field>
                <Field label="Agent's routed balance" hint="inside the router, not its USDC">
                  {meter.routedBalanceUsd ? (
                    <UsdAmount value={meter.routedBalanceUsd} className="text-base font-semibold" />
                  ) : (
                    <span className="text-base font-semibold text-muted-foreground">—</span>
                  )}
                </Field>
              </div>
            </div>

            {meter.overFloatCap && (
              <Notice tone="warn">
                Float is above the cap. The cap is checked on deposits only; value arriving from a
                counterparty raises the float without one, because refusing an inbound payment would
                make an honest agent unpayable.
              </Notice>
            )}
          </>
        )}
      </CardContent>

      <CardFooter className="text-xs text-muted-foreground">
        <span>
          Routed spend is cumulative gross flow, not loss — a dollar shuttled between two addresses
          one operator controls drives it past any bound for the price of gas. The float cap is the
          separate number that bounds real exposure: it is the most the operator lets the router
          hold for this certificate, and therefore the most a stolen agent key can reach.
        </span>
      </CardFooter>
    </Card>
  );
}
