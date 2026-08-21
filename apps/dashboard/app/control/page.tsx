"use client";
// /control — the proof cockpit. Two ways to drive the same protocol:
//  · "Run the proof (demo keys)" — deterministic, server-signed: seed → fund the
//    trap → challenge → watch the slash. Plus the adversarial cheat lane.
//  · "Play a role with your wallet" — a connected wallet signs the open actions
//    itself (stake / attest / publish / fee / pay / challenge), funds routed to
//    and from its own address.
// Every action refetches the ledger, so each click produces a visible state diff.
import { useCallback } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Badge } from "@/components/ui/badge";
import { LedgerBoard } from "@/app/components/control/LedgerBoard";
import { CoveragePanel } from "@/app/components/CoveragePanel";
import { SpendMeter } from "@/app/components/SpendMeter";
import { ActionButton, type ActionResult } from "@/app/components/control/ActionButton";
import { AddressPill } from "@/app/components/AddressPill";
import { useLedger } from "@/app/lib/hooks/useLedger";
import { useCoverage } from "@/app/lib/hooks/useCoverage";
import { useWallet } from "@/app/lib/wallet/useWallet";
import { useWalletActions } from "@/app/lib/wallet/actions";
import { roles, roleForAddress } from "@/app/lib/ui-config";
import type { WalletAction, BuildParams } from "@/app/lib/tx-build";

async function post<T = Record<string, unknown>>(url: string, body: unknown = {}): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data?.error ?? `${url} failed (${res.status})`);
  return data as T;
}

function Lane({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div>
        <div className="text-sm font-medium">{title}</div>
        {hint && <div className="text-xs text-muted-foreground">{hint}</div>}
      </div>
      <div className="flex flex-wrap gap-2">{children}</div>
    </div>
  );
}

export default function ControlPage() {
  const { ledger, loading, refetch } = useLedger(5000);
  const { coverage, refetch: refetchCoverage } = useCoverage(roles.agent.address, { pollMs: 5000 });
  const { address } = useWallet();
  const wallet = useWalletActions();

  // Every action on this page can move both boards: paying a premium changes the
  // operator's and the treasury's balances as well as the coverage record, and a
  // slash closes the coverage at the same instant it flips the certificate. One
  // refresh for both keeps them from disagreeing for a poll interval.
  const refreshAll = useCallback(() => {
    refetch();
    void refetchCoverage();
  }, [refetch, refetchCoverage]);

  const certId = ledger?.cert.certId ?? null;
  const noCert = certId == null;

  // wallet action → ActionResult (message + tx hash), for ActionButton
  const w =
    (action: WalletAction, params: BuildParams, message: (result: unknown) => string) =>
    async (): Promise<ActionResult> => {
      const { hash, result } = await wallet.run(action, params);
      return { message: message(result), txHash: hash };
    };

  const connectedRole = address ? roleForAddress(address) : null;

  return (
    <main className="mx-auto max-w-5xl space-y-6 px-4 py-8">
      <div className="space-y-1">
        <h1 className="text-2xl font-semibold tracking-tight">Proof cockpit</h1>
        <p className="text-sm text-muted-foreground">
          Drive all five actors on live testnet. Value moves between named accounts, and every
          defection is punished on-chain. Run it with demo keys, or play a role with your own
          wallet.
        </p>
      </div>

      <LedgerBoard ledger={ledger} loading={loading} />

      {coverage && (
        <div className="grid gap-6 lg:grid-cols-2">
          <CoveragePanel view={coverage} />
          <SpendMeter view={coverage} />
        </div>
      )}

      <div className="grid gap-6 lg:grid-cols-2">
        {/* ── Section B: deterministic demo (server keys) ───────────────── */}
        <Card>
          <CardHeader>
            <CardTitle>Run the proof</CardTitle>
            <CardDescription>
              Server-signed demo keys — the deterministic happy path + climax.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            <Lane title="Reset" hint="Re-seed a fresh Verified cert (re-stakes the auditor).">
              <ActionButton
                label="Seed / Reset cert"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ certId: number }>("/api/auditor", {
                    action: "sign-publish",
                  });
                  return `Seeded cert #${d.certId} → Verified`;
                }}
              />
            </Lane>

            <Separator />

            <Lane
              title="Operator"
              hint="Fund the reserve below the $10k claim to set the trap, or at/above for the honest path."
            >
              <ActionButton
                label="Fund $4k (trap)"
                variant="secondary"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ reserveHeldUsd: string; claimedUsd: string }>(
                    "/api/operator",
                    {
                      action: "deposit-reserve",
                      amountUsd: 4000,
                    },
                  );
                  return `Reserve → ${d.reserveHeldUsd} (claims ${d.claimedUsd})`;
                }}
              />
              <ActionButton
                label="Fund $10k (honest)"
                variant="secondary"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ reserveHeldUsd: string; claimedUsd: string }>(
                    "/api/operator",
                    {
                      action: "deposit-reserve",
                      amountUsd: 10000,
                    },
                  );
                  return `Reserve → ${d.reserveHeldUsd} (claims ${d.claimedUsd})`;
                }}
              />
              <ActionButton
                label="Deposit fee $500"
                variant="outline"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ feeUsd: string }>("/api/operator", {
                    action: "deposit-fee",
                  });
                  return `Fee escrowed ${d.feeUsd}`;
                }}
              />
            </Lane>

            <Separator />

            <Lane
              title="Coverage premium"
              hint="Priced on bound × term, fixed at publish. Payable once, and only on a Verified cert — the yield needs an auditor to accrue to."
            >
              <ActionButton
                label="Operator pays premium"
                variant="secondary"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{
                    hash?: string;
                    premiumUsd: string;
                    protocolFeeUsd: string;
                    yieldPotUsd: string;
                  }>("/api/coverage", { action: "pay-premium" });
                  return {
                    message: `Premium ${d.premiumUsd} paid — ${d.protocolFeeUsd} protocol fee, ${d.yieldPotUsd} accruing to the auditor`,
                    txHash: d.hash,
                  };
                }}
              />
              <ActionButton
                label="Auditor claims yield"
                variant="outline"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ hash?: string; claimedUsd: string }>("/api/coverage", {
                    action: "claim-premium",
                  });
                  // claim() returns 0 rather than reverting when nothing has
                  // accrued yet, so say that instead of reporting a claim of $0.
                  return {
                    message:
                      d.claimedUsd === "$0"
                        ? "Nothing accrued yet — no yield to withdraw"
                        : `Auditor withdrew ${d.claimedUsd} of accrued yield`,
                    txHash: d.hash,
                  };
                }}
              />
            </Lane>

            <Separator />

            <Lane
              title="Spend meter"
              hint="Only an enrolled agent's payments reach the counter. A binding is permanent, so re-seeding the cert will not move it onto the new one."
            >
              <ActionButton
                label="Enroll agent (cap $2,000)"
                variant="outline"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ certId: number; floatCapUsd: string }>("/api/coverage", {
                    action: "enroll-agent",
                  });
                  return `Agent enrolled on cert #${d.certId} · float cap ${d.floatCapUsd}`;
                }}
              />
            </Lane>

            <Separator />

            <Lane
              title="Challenger — the climax"
              hint="resolve() proves fraud by arithmetic. Slash if reserve is short; bond forfeit if funded."
            >
              <ActionButton
                label="Challenge → resolve"
                variant="destructive"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{
                    outcome: string;
                    victimUsdBefore: string;
                    victimUsdAfter: string;
                  }>("/api/challenger");
                  return `${d.outcome} · victim ${d.victimUsdBefore} → ${d.victimUsdAfter}`;
                }}
              />
            </Lane>

            <Separator />

            <Lane
              title="Cheat lane"
              hint="Each defection is simulated and EXPECTED to revert — the revert is the proof (no funds move)."
            >
              <ActionButton
                label="Withdraw reserve early"
                variant="outline"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ reverted: boolean; reason?: string; expected?: string }>(
                    "/api/cheat",
                    {
                      action: "withdraw-reserve",
                    },
                  );
                  return d.reverted
                    ? `REVERTED ✓ — ${d.expected ?? d.reason}`
                    : "⚠ did NOT revert — lock failed";
                }}
              />
              <ActionButton
                label="Withdraw stake early"
                variant="outline"
                onDone={refreshAll}
                run={async () => {
                  const d = await post<{ reverted: boolean; reason?: string; expected?: string }>(
                    "/api/cheat",
                    {
                      action: "withdraw-stake",
                    },
                  );
                  return d.reverted
                    ? `REVERTED ✓ — ${d.expected ?? d.reason}`
                    : "⚠ did NOT revert — lock failed";
                }}
              />
            </Lane>
          </CardContent>
        </Card>

        {/* ── Section C: play a role with your wallet ───────────────────── */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between gap-2">
              <CardTitle>Play a role with your wallet</CardTitle>
              {address && <Badge variant="outline">{connectedRole ?? "connected"}</Badge>}
            </div>
            <CardDescription>
              {address ? (
                <span className="inline-flex items-center gap-2">
                  Signing as <AddressPill address={address} showRole />
                </span>
              ) : (
                "Connect a wallet (top right) to sign these actions yourself."
              )}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-5">
            <Lane title="Get ready" hint="Fund a fresh wallet: XLM + USDC trustline + test USDC.">
              <ActionButton
                label="Fund my wallet"
                variant="secondary"
                disabled={!address}
                onDone={refreshAll}
                run={async () => {
                  const d = await wallet.fund(5000);
                  return `Funded → ${d.balanceUsd} USDC`;
                }}
              />
            </Lane>

            <Separator />

            <Lane title="Auditor" hint="Stake your own slashable capital, then attest the cert.">
              <ActionButton
                label="Stake $1,500"
                disabled={!address}
                onDone={refreshAll}
                run={w("stake", { amountUsd: 1500 }, () => "Staked $1,500")}
              />
              <ActionButton
                label="Attest cert"
                variant="outline"
                disabled={!address || noCert}
                title={noCert ? "No cert to attest — seed one first" : undefined}
                onDone={refreshAll}
                run={w("attest", { certId: certId ?? undefined }, () => `Attested cert #${certId}`)}
              />
            </Lane>

            <Separator />

            <Lane
              title="Operator"
              hint="Publish a cert and escrow the fee (reserve stays server-side by design)."
            >
              <ActionButton
                label="Publish cert"
                variant="outline"
                disabled={!address}
                onDone={refreshAll}
                run={w(
                  "publish",
                  { agent: roles.agent.address },
                  (r) => `Published cert #${r} (Pending)`,
                )}
              />
              <ActionButton
                label="Deposit fee $500"
                variant="outline"
                disabled={!address}
                onDone={refreshAll}
                run={w(
                  "deposit-fee",
                  { auditor: roles.auditor.address, amountUsd: 500 },
                  () => "Fee escrowed $500",
                )}
              />
            </Lane>

            <Separator />

            <Lane title="Agent" hint="Pay a counterparty directly in USDC.">
              <ActionButton
                label="Pay $500 → Counterparty"
                disabled={!address}
                onDone={refreshAll}
                run={w(
                  "pay",
                  { to: roles.counterparty.address, amountUsd: 500 },
                  () => "Paid $500 → Counterparty",
                )}
              />
            </Lane>

            <Separator />

            <Lane
              title="Challenger"
              hint="Post a $100 bond and trigger the slash — the finder's fee lands in your wallet."
            >
              <ActionButton
                label="Challenge → resolve"
                variant="destructive"
                disabled={!address || noCert}
                title={noCert ? "No cert to challenge — seed one first" : undefined}
                onDone={refreshAll}
                run={w(
                  "challenge",
                  { certId: certId ?? undefined, victim: roles.counterparty.address, bondUsd: 100 },
                  () => "Challenge submitted & resolved",
                )}
              />
            </Lane>
          </CardContent>
        </Card>
      </div>
    </main>
  );
}
