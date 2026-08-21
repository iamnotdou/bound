// A labelled figure with an optional one-line hint underneath — the layout the
// CertificateCard established for every number a counterparty reads. Extracted
// so the coverage and metering panels render at the same rhythm as the card they
// sit beside, rather than each inventing their own spacing.
export function Field({
  label,
  children,
  hint,
}: {
  label: string;
  children: React.ReactNode;
  hint?: string;
}) {
  return (
    <div className="space-y-1">
      <div className="text-xs font-medium text-muted-foreground">{label}</div>
      <div className="text-sm">{children}</div>
      {hint && <div className="text-[11px] text-muted-foreground">{hint}</div>}
    </div>
  );
}
