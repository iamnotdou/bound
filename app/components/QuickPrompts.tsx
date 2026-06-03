"use client";
// Preset scenario buttons for /chat. Each one is seeded with the live cert id +
// the demo addresses so a judge can drive the whole protocol from one click:
// read the cert → pay → pay-per-use via x402 → challenge the false vouch. The
// prompts are phrased so the agent picks the matching tool on its own.
import { ShieldCheck, Banknote, Globe, Gavel } from "lucide-react";
import { Button } from "@/components/ui/button";
import { roles } from "@/app/lib/ui-config";

export interface QuickPrompt {
  label: string;
  hint: string;
  prompt: string;
  Icon: React.ComponentType<{ className?: string }>;
  destructive?: boolean;
  disabled?: boolean;
}

export function buildQuickPrompts(certId: number | null): QuickPrompt[] {
  const agent = roles.agent.address;
  const counterparty = roles.counterparty.address;
  // x402 fetch runs server-side, so it needs an absolute URL — build it from the
  // current origin (client-only component).
  const origin = typeof window !== "undefined" ? window.location.origin : "";
  const serviceUrl = `${origin}/api/paid-service?price=120`;

  return [
    {
      label: "Verify",
      hint: "read the certificate",
      Icon: ShieldCheck,
      prompt: `Before I transact with this agent, verify its Bound Certificate and tell me my worst-case loss. Agent: ${agent}`,
    },
    {
      label: "Pay $500",
      hint: "direct USDC",
      Icon: Banknote,
      prompt: `Pay $500 USDC to the counterparty at ${counterparty} for the data API service.`,
    },
    {
      label: "x402",
      hint: "pay-per-use",
      Icon: Globe,
      prompt: `Access the premium market-data service at ${serviceUrl} and autonomously pay whatever it charges via x402, then show me the data.`,
    },
    {
      label: "Challenge",
      hint: certId != null ? `cert #${certId}` : "no cert yet",
      Icon: Gavel,
      destructive: true,
      disabled: certId == null,
      prompt:
        certId != null
          ? `The certificate (#${certId}) claims a $10,000 reserve. Challenge it on-chain to prove whether the reserve is actually there. If it's short, slash the auditor and compensate the victim — the counterparty at ${counterparty}.`
          : "",
    },
  ];
}

export function QuickPrompts({
  certId,
  onPick,
  disabled,
}: {
  certId: number | null;
  onPick: (prompt: string) => void;
  disabled?: boolean;
}) {
  const prompts = buildQuickPrompts(certId);
  return (
    <div className="flex flex-wrap gap-2">
      {prompts.map((p) => (
        <Button
          key={p.label}
          type="button"
          size="sm"
          variant={p.destructive ? "outline" : "secondary"}
          disabled={disabled || p.disabled}
          title={p.disabled ? "Seed a certificate first (Auditor or Control page)" : p.hint}
          onClick={() => onPick(p.prompt)}
          className={p.destructive ? "border-red-500/40 text-red-600 hover:bg-red-500/10 dark:text-red-400" : ""}
        >
          <p.Icon className="size-3.5" />
          {p.label}
          <span className="hidden text-xs text-muted-foreground sm:inline">· {p.hint}</span>
        </Button>
      ))}
    </div>
  );
}
