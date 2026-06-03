"use client";
// Truncated Stellar address / contract id with copy + stellar.expert link.
import { useState } from "react";
import { Copy, Check, ExternalLink } from "lucide-react";
import { truncate, roleForAddress } from "@/app/lib/ui-config";
import { explorer } from "@/app/lib/explorer";
import { cn } from "@/lib/utils";

export function AddressPill({
  address,
  kind = "account",
  showRole = false,
  className,
}: {
  address: string;
  kind?: "account" | "contract";
  showRole?: boolean;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  if (!address) return null;

  const href = kind === "contract" ? explorer.contract(address) : explorer.account(address);
  const role = showRole ? roleForAddress(address) : null;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(address);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard blocked — no-op */
    }
  };

  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 rounded-md border bg-muted/40 px-2 py-0.5 font-mono text-xs",
        className,
      )}
    >
      {role && <span className="font-sans text-muted-foreground">{role}</span>}
      <span title={address}>{truncate(address)}</span>
      <button
        onClick={copy}
        className="text-muted-foreground transition-colors hover:text-foreground"
        aria-label="Copy address"
      >
        {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
      </button>
      <a
        href={href}
        target="_blank"
        rel="noreferrer"
        className="text-muted-foreground transition-colors hover:text-foreground"
        aria-label="View on stellar.expert"
      >
        <ExternalLink className="size-3" />
      </a>
    </span>
  );
}
