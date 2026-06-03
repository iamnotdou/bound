// Renders a USD amount. Accepts either a pre-formatted string the API returns
// ("$10,000") or a raw dollar number. Server-safe (no client hooks).
import { cn } from "@/lib/utils";

export function UsdAmount({
  value,
  className,
  muted,
}: {
  value: string | number;
  className?: string;
  muted?: boolean;
}) {
  const text =
    typeof value === "number"
      ? `$${value.toLocaleString(undefined, { maximumFractionDigits: 2 })}`
      : value;
  return (
    <span className={cn("font-mono tabular-nums", muted && "text-muted-foreground", className)}>
      {text}
    </span>
  );
}
