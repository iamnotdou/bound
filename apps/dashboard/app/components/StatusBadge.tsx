// Certificate status pill. Maps the on-chain status tag to a color. The cert's
// lifecycle is Pending (operator published) → Verified (auditor attested) →
// Invalid (challenged & slashed, or never valid).
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";

const STYLES: Record<string, string> = {
  Verified: "border-transparent bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
  Pending: "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400",
  Invalid: "border-transparent bg-red-500/15 text-red-600 dark:text-red-400",
};

export function StatusBadge({ status, className }: { status: string; className?: string }) {
  return (
    <Badge variant="outline" className={cn("gap-1.5", STYLES[status] ?? "", className)}>
      <span className="size-1.5 rounded-full bg-current" />
      {status}
    </Badge>
  );
}
