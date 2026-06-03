// Placeholder landing — replaced by the real landing in a later step. Exists so
// the app has a root route and we can confirm Tailwind compiles end to end.
export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-4 p-8 text-center">
      <h1 className="text-3xl font-semibold tracking-tight">Bound Protocol</h1>
      <p className="max-w-md text-sm text-muted-foreground">
        A surety bond for AI agents on Stellar. Frontend scaffold is live —
        pages are being built.
      </p>
    </main>
  );
}
