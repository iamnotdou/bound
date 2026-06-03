"use client";
// /chat — the judged centerpiece. An operator (human) talks to the Bound agent;
// the agent runs server-side tools (verify / pay / x402 / challenge) and every
// tool call renders as a state-colored ToolCallCard inline in the transcript.
// A live certificate panel polls the chain and flips Verified → Invalid the
// instant a challenge slashes the auditor — the climax, shown not told.
//
// All signing stays server-side: this page only POSTs messages to /api/chat.
import { useEffect, useRef } from "react";
import { useChat } from "ai/react";
import { Send, Square, Bot } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { CertificateCard } from "@/app/components/CertificateCard";
import { QuickPrompts } from "@/app/components/QuickPrompts";
import { ToolCallCard, type ToolInvocation } from "@/app/components/ToolCallCard";
import { useCert } from "@/app/lib/hooks/useCert";
import { roles } from "@/app/lib/ui-config";
import { cn } from "@/lib/utils";

export default function ChatPage() {
  const agent = roles.agent.address;
  const { cert, loading: certLoading, refetch } = useCert(agent, { pollMs: 5000 });
  const certId = cert?.certId ?? null;

  const { messages, input, handleInputChange, handleSubmit, append, status, stop, error } =
    useChat({
      api: "/api/chat",
      // a tool call may have mutated chain state (a slash) — re-read the cert the
      // moment the agent finishes its turn so the panel flips without poll lag.
      onFinish: () => void refetch(),
    });

  const busy = status === "submitted" || status === "streaming";

  // keep the transcript pinned to the newest message
  const endRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages, status]);

  return (
    <main className="mx-auto grid max-w-6xl gap-6 px-4 py-8 lg:grid-cols-[minmax(0,1fr)_22rem]">
      {/* ── transcript ─────────────────────────────────────────────── */}
      <section className="flex min-h-[70vh] flex-col">
        <div className="space-y-1">
          <h1 className="text-2xl font-semibold tracking-tight">Talk to the agent</h1>
          <p className="text-sm text-muted-foreground">
            An autonomous payment agent with no spending cap. What keeps you safe is its
            certificate — read it, pay it, then prove a false vouch on-chain.
          </p>
        </div>

        <div className="mt-4 flex-1 space-y-4 overflow-y-auto pr-1">
          {messages.length === 0 && (
            <div className="rounded-xl border border-dashed p-6 text-sm text-muted-foreground">
              Ask the agent to verify its certificate, make a payment, pay a service via x402,
              or challenge a false attestation. Use a quick prompt below to start.
            </div>
          )}

          {messages.map((m) => {
            const isUser = m.role === "user";
            const parts =
              m.parts && m.parts.length > 0
                ? m.parts
                : ([{ type: "text", text: m.content }] as typeof m.parts);

            return (
              <div key={m.id} className={cn("flex gap-3", isUser && "flex-row-reverse")}>
                {!isUser && (
                  <div className="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-full bg-muted">
                    <Bot className="size-4" />
                  </div>
                )}
                <div className={cn("min-w-0 max-w-[85%] space-y-2", isUser && "flex flex-col items-end")}>
                  {parts!.map((part, i) => {
                    if (part.type === "text") {
                      if (!part.text) return null;
                      return (
                        <div
                          key={i}
                          className={cn(
                            "whitespace-pre-wrap rounded-2xl px-3.5 py-2 text-sm",
                            isUser
                              ? "bg-primary text-primary-foreground"
                              : "bg-muted text-foreground",
                          )}
                        >
                          {part.text}
                        </div>
                      );
                    }
                    if (part.type === "tool-invocation") {
                      return (
                        <ToolCallCard
                          key={i}
                          inv={part.toolInvocation as unknown as ToolInvocation}
                        />
                      );
                    }
                    return null;
                  })}
                </div>
              </div>
            );
          })}

          {status === "submitted" && (
            <div className="flex items-center gap-2 pl-10 text-sm text-muted-foreground">
              <span className="size-1.5 animate-pulse rounded-full bg-current" />
              Thinking…
            </div>
          )}
          {error && (
            <div className="rounded-lg border border-red-500/40 bg-red-500/[0.04] px-3 py-2 text-sm text-red-700 dark:text-red-400">
              {error.message.includes("ANTHROPIC")
                ? "ANTHROPIC_API_KEY is not set — add it to .env.testnet to enable the chat."
                : `Chat error: ${error.message}`}
            </div>
          )}
          <div ref={endRef} />
        </div>

        {/* ── composer ─────────────────────────────────────────────── */}
        <div className="mt-4 space-y-3 border-t pt-4">
          <QuickPrompts
            certId={certId}
            disabled={busy}
            onPick={(prompt) => void append({ role: "user", content: prompt })}
          />
          <form onSubmit={handleSubmit} className="flex gap-2">
            <Input
              value={input}
              onChange={handleInputChange}
              placeholder="Message the agent…"
              disabled={busy}
            />
            {busy ? (
              <Button type="button" variant="outline" onClick={stop}>
                <Square className="size-4" /> Stop
              </Button>
            ) : (
              <Button type="submit" disabled={!input.trim()}>
                <Send className="size-4" /> Send
              </Button>
            )}
          </form>
        </div>
      </section>

      {/* ── live certificate panel ─────────────────────────────────── */}
      <aside className="lg:sticky lg:top-20 lg:self-start">
        <div className="space-y-2">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-medium">Live certificate</h2>
            <span className="text-xs text-muted-foreground">polls on-chain</span>
          </div>
          {certLoading && !cert ? (
            <Skeleton className="h-64 w-full rounded-xl" />
          ) : cert ? (
            <CertificateCard cert={cert} />
          ) : (
            <p className="text-sm text-muted-foreground">Reading the agent&apos;s certificate…</p>
          )}
          <p className="text-xs text-muted-foreground">
            This panel reads the live testnet certificate for the demo agent. When a challenge
            proves the reserve is short, the auditor is slashed and the status flips to{" "}
            <span className="font-medium text-red-600 dark:text-red-400">Invalid</span> — right here.
          </p>
        </div>
      </aside>
    </main>
  );
}
