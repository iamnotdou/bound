// near-api-js's FailoverRpcProvider prints every failed attempt with
// console.error(errorObject) before it retries or fails over (a -429 from the
// free RPC tier, or AccountDoesNotExist while polling for a new account).
// The call itself still succeeds or throws normally, so those stack traces
// only look like crashes. `quietRpcRetryLogs()` drops bare Error objects from
// console.error unless BOUND_NEAR_DEBUG=1. Opt-in: importing the library
// never patches console; the CLI and the workspace scripts call it.

let patched = false;

export function quietRpcRetryLogs(): void {
  if (patched || process.env.BOUND_NEAR_DEBUG) return;
  patched = true;
  const original = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    if (args.length === 1 && args[0] instanceof Error) return;
    original(...args);
  };
}
