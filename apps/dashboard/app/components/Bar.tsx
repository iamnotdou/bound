// A one-line ratio bar. Used for accrual (a fraction of a pot that cannot
// exceed 1) and for the spend meter (a fraction that very much can).
//
// The fill is clamped to the track, but an over-full bar is coloured rather than
// silently drawn at 100%: the panels beside it depend on a reader being able to
// see the difference between "at the limit" and "past it".
import { cn } from "@/lib/utils";

const TONES = {
  neutral: "bg-foreground/40",
  good: "bg-emerald-500",
  warn: "bg-amber-500",
  over: "bg-red-500",
} as const;

export function Bar({
  value,
  tone = "neutral",
  className,
}: {
  /** 0..1, or beyond 1 when the measured quantity has passed its reference. */
  value: number | null;
  tone?: keyof typeof TONES;
  className?: string;
}) {
  const pct = value === null ? 0 : Math.min(Math.max(value, 0), 1) * 100;
  return (
    <div
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-muted", className)}
      role="presentation"
    >
      <div
        className={cn("h-full rounded-full transition-[width] duration-500", TONES[tone])}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}
