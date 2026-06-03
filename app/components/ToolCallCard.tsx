"use client";
// Renders one agent tool call as a state-colored card in the /chat transcript.
// The tool's lifecycle drives the color: a call still in flight is neutral with
// a spinner; a settled result is green (executed cleanly), amber (a challenge
// that didn't slash), or red (a slash / an invalidated certificate) — so the
// climax reads at a glance. The result shapes mirror exactly what each tool's
// execute() returns in app/lib/agent-tools.ts.
import {
  ShieldCheck,
  Banknote,
  Globe,
  Gavel,
  LoaderCircle,
  CircleCheck,
  CircleX,
  Zap,
  ExternalLink,
} from "lucide-react";
import { explorer } from "@/app/lib/explorer";
import { truncate } from "@/app/lib/ui-config";
import { cn } from "@/lib/utils";

// The AI SDK v4 tool-invocation shape (message.parts[].toolInvocation).
export interface ToolInvocation {
  state: "partial-call" | "call" | "result";
  toolName: string;
  toolCallId: string;
  args?: Record<string, unknown>;
  result?: unknown;
}

type IconType = React.ComponentType<{ className?: string }>;

const META: Record<string, { Icon: IconType }> = {
  verify_agent_certificate: { Icon: ShieldCheck },
  execute_payment: { Icon: Banknote },
  fetch_paid_service: { Icon: Globe },
  challenge_certificate: { Icon: Gavel },
  get_balance: { Icon: Banknote },
};

type Tone = "pending" | "ok" | "warn" | "bad";

const TONE: Record<Tone, { card: string; chip: string; Icon: IconType }> = {
  pending: { card: "border-border", chip: "bg-muted text-muted-foreground", Icon: LoaderCircle },
  ok: {
    card: "border-emerald-500/40 bg-emerald-500/[0.03]",
    chip: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-400",
    Icon: CircleCheck,
  },
  warn: {
    card: "border-amber-500/40 bg-amber-500/[0.03]",
    chip: "bg-amber-500/15 text-amber-700 dark:text-amber-400",
    Icon: Zap,
  },
  bad: {
    card: "border-red-500/40 bg-red-500/[0.03]",
    chip: "bg-red-500/15 text-red-700 dark:text-red-400",
    Icon: CircleX,
  },
};

function KV({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="shrink-0 text-xs text-muted-foreground">{k}</span>
      <span className="min-w-0 truncate text-right text-sm">{children}</span>
    </div>
  );
}

function TxLink({ hash }: { hash?: string | null }) {
  if (!hash) return <span className="text-muted-foreground">—</span>;
  return (
    <a
      href={explorer.tx(hash)}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1 font-mono text-xs text-foreground underline-offset-2 hover:underline"
    >
      {truncate(hash, 6, 6)}
      <ExternalLink className="size-3" />
    </a>
  );
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span className="font-mono text-xs">{children}</span>;
}

// Decide tone + body purely from the settled result of each tool.
function renderResult(toolName: string, result: any): { tone: Tone; body: React.ReactNode } {
  switch (toolName) {
    case "verify_agent_certificate": {
      const valid = !!result?.valid;
      const status = String(result?.status ?? "—");
      const tone: Tone = status === "Invalid" ? "bad" : valid ? "ok" : "warn";
      return {
        tone,
        body: (
          <div className="space-y-1.5">
            <KV k="status">
              <span
                className={cn(
                  "font-medium",
                  status === "Verified" && "text-emerald-700 dark:text-emerald-400",
                  status === "Invalid" && "text-red-700 dark:text-red-400",
                  status === "Pending" && "text-amber-700 dark:text-amber-400",
                )}
              >
                {valid ? "VALID" : status.toUpperCase()}
              </span>
            </KV>
            <KV k="bound (worst case)"><Mono>{result?.bound ?? "—"}</Mono></KV>
            <KV k="reserve (locked)"><Mono>{result?.reserve ?? "—"}</Mono></KV>
            <KV k="auditor stake (slashable)"><Mono>{result?.auditorStake ?? "—"}</Mono></KV>
            {result?.auditor && <KV k="auditor"><Mono>{truncate(result.auditor)}</Mono></KV>}
          </div>
        ),
      };
    }

    case "execute_payment": {
      const ok = !!result?.success;
      return {
        tone: ok ? "ok" : "bad",
        body: (
          <div className="space-y-1.5">
            <KV k="amount">
              <span className="font-mono font-semibold">
                ${Number(result?.amountUsd ?? 0).toLocaleString()}
              </span>
            </KV>
            <KV k="recipient"><Mono>{truncate(String(result?.recipient ?? "—"))}</Mono></KV>
            <KV k="tx"><TxLink hash={result?.txHash} /></KV>
          </div>
        ),
      };
    }

    case "fetch_paid_service": {
      const paid = result?.paid as { amountUsd?: number; recipient?: string; txHash?: string } | null;
      let bodyText = "";
      try {
        const parsed = typeof result?.body === "string" ? JSON.parse(result.body) : result?.body;
        bodyText = parsed?.data ?? (typeof result?.body === "string" ? result.body : "");
      } catch {
        bodyText = typeof result?.body === "string" ? result.body : "";
      }
      return {
        tone: "ok",
        body: (
          <div className="space-y-1.5">
            <KV k="http status"><Mono>{result?.status ?? "—"}</Mono></KV>
            {paid ? (
              <>
                <KV k="paid (x402)">
                  <span className="font-mono font-semibold">
                    ${Number(paid.amountUsd ?? 0).toLocaleString()}
                  </span>
                </KV>
                <KV k="tx"><TxLink hash={paid.txHash} /></KV>
              </>
            ) : (
              <KV k="payment"><span className="text-muted-foreground">none required</span></KV>
            )}
            {bodyText && (
              <p className="border-t pt-1.5 text-xs text-muted-foreground">{bodyText}</p>
            )}
          </div>
        ),
      };
    }

    case "challenge_certificate": {
      const slashed = result?.certStatusAfter === "Invalid";
      return {
        tone: slashed ? "bad" : "warn",
        body: (
          <div className="space-y-1.5">
            {slashed && (
              <div className="flex items-center gap-1.5 rounded-md bg-red-500/10 px-2 py-1 text-xs font-semibold text-red-700 dark:text-red-400">
                <Zap className="size-3.5" /> FRAUD PROVEN — by arithmetic
              </div>
            )}
            <KV k="challenge id"><Mono>#{result?.challengeId ?? "—"}</Mono></KV>
            <KV k="cert status">
              <span className="font-mono">
                {result?.certStatusBefore ?? "—"} → {result?.certStatusAfter ?? "—"}
              </span>
            </KV>
            <p className="border-t pt-1.5 text-xs text-muted-foreground">{result?.outcome ?? ""}</p>
          </div>
        ),
      };
    }

    case "get_balance": {
      return {
        tone: "ok",
        body: (
          <div className="space-y-1.5">
            <KV k="address"><Mono>{truncate(String(result?.address ?? "—"))}</Mono></KV>
            <KV k="balance"><span className="font-mono font-semibold">{result?.balance ?? "—"}</span></KV>
          </div>
        ),
      };
    }

    default:
      return {
        tone: "ok",
        body: (
          <pre className="overflow-x-auto text-xs text-muted-foreground">
            {JSON.stringify(result, null, 2)}
          </pre>
        ),
      };
  }
}

export function ToolCallCard({ inv }: { inv: ToolInvocation }) {
  const running = inv.state !== "result";
  const { Icon } = META[inv.toolName] ?? { Icon: ShieldCheck };

  const { tone, body } = running
    ? { tone: "pending" as Tone, body: null }
    : renderResult(inv.toolName, inv.result);

  const t = TONE[tone];
  const StatusIcon = t.Icon;

  return (
    <div className={cn("rounded-lg border px-3 py-2.5 text-sm", t.card)}>
      <div className="flex items-center gap-2">
        <Icon className="size-4 text-muted-foreground" />
        <span className="font-mono text-xs font-medium">{inv.toolName}</span>
        <span
          className={cn(
            "ml-auto inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium",
            t.chip,
          )}
        >
          <StatusIcon className={cn("size-3", running && "animate-spin")} />
          {running ? "running" : tone === "bad" ? "slashed" : tone === "warn" ? "challenged" : "done"}
        </span>
      </div>
      {body && <div className="mt-2.5">{body}</div>}
    </div>
  );
}
