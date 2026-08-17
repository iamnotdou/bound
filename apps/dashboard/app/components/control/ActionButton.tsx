"use client";
// A button that runs one async actor action: shows a spinner, toasts the result
// (with a "View tx" link when a hash is returned), and refreshes the ledger so
// the state diff is visible. Errors surface as a toast; the button never throws.
import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { explorer } from "@/app/lib/explorer";

export interface ActionResult {
  message: string;
  txHash?: string;
}

export function ActionButton({
  label,
  run,
  onDone,
  variant = "default",
  size = "sm",
  disabled,
  title,
  className,
}: {
  label: string;
  run: () => Promise<ActionResult | string | void>;
  onDone?: () => void;
  variant?: "default" | "outline" | "secondary" | "destructive" | "ghost";
  size?: "sm" | "default";
  disabled?: boolean;
  title?: string;
  className?: string;
}) {
  const [loading, setLoading] = useState(false);
  return (
    <Button
      variant={variant}
      size={size}
      disabled={loading || disabled}
      title={title}
      className={className}
      onClick={async () => {
        setLoading(true);
        try {
          const r = await run();
          if (r) {
            const res = typeof r === "string" ? { message: r } : r;
            toast.success(res.message, {
              ...(res.txHash
                ? {
                    description: `tx ${res.txHash.slice(0, 12)}…`,
                    action: {
                      label: "View",
                      onClick: () => window.open(explorer.tx(res.txHash!), "_blank"),
                    },
                  }
                : {}),
            });
          }
        } catch (e) {
          toast.error((e as Error).message);
        } finally {
          setLoading(false);
          onDone?.();
        }
      }}
    >
      {loading ? "…" : label}
    </Button>
  );
}
