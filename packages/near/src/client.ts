// Typed wrappers over the Bound NEAR contract, using near-api-js v7 only.
// Reads go to FastNEAR with rpc.testnet.near.org as fallback, at `final`
// finality, retried 3 times on transient errors. Writes wait for FINAL and
// never auto-resend (a resend could double-apply a payment).

import {
  Account,
  FailoverRpcProvider,
  JsonRpcProvider,
  KeyPair,
  KeyPairSigner,
  actions,
  type KeyPairString,
  type Provider,
} from "near-api-js";
import { explorerTx } from "./format.js";

// rpc.testnet.near.org now answers every call with -429 "THIS ENDPOINT IS
// DEPRECATED" (checked 2026-11-13), so both entries are FastNEAR hostnames.
export const TESTNET_RPCS = ["https://test.rpc.fastnear.com", "https://rpc.testnet.fastnear.com"];

// Regular nodes garbage-collect transactions after a few epochs (about 2.5
// days on testnet), and judging runs after the demo. Transaction lookups in
// near:evidence ask an archival node first.
export const TESTNET_ARCHIVAL_RPCS = ["https://archival-rpc.testnet.fastnear.com", ...TESTNET_RPCS];

export const TGAS = 1_000_000_000_000n;
export const ONE_YOCTO = 1n;
export const MILLINEAR = 10n ** 21n;

// near-api-js's FailoverRpcProvider prints every failed attempt with
// console.error(errorObject) before it retries or fails over (a -429 from the
// free RPC tier, or AccountDoesNotExist while polling for a new account).
// The call itself still succeeds or throws normally, so those stack traces
// only look like crashes. Drop bare Error objects from console.error unless
// BOUND_NEAR_DEBUG=1; our own scripts always log strings.
if (!process.env.BOUND_NEAR_DEBUG) {
  const original = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    if (args.length === 1 && args[0] instanceof Error) return;
    original(...args);
  };
}

export function testnetProvider(urls: string[] = TESTNET_RPCS): Provider {
  return new FailoverRpcProvider(urls.map((url) => new JsonRpcProvider({ url })));
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function withRetry<T>(fn: () => Promise<T>, tries = 3): Promise<T> {
  let last: unknown;
  for (let i = 0; i < tries; i++) {
    try {
      return await fn();
    } catch (e) {
      last = e;
      await sleep(800 * (i + 1));
    }
  }
  throw last;
}

export async function view<T>(
  provider: Provider,
  contractId: string,
  method: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return withRetry(async () => {
    const r = await provider.callFunction({
      contractId,
      method,
      args,
      blockQuery: { finality: "final" },
    } as never);
    return r as T;
  });
}

export function accountFromKey(provider: Provider, accountId: string, key: KeyPairString): Account {
  return new Account(accountId, provider, new KeyPairSigner(KeyPair.fromString(key)));
}

/** Result of one on-chain step. `hash` is null when the transaction never existed
 *  (for example the network refused the access key before execution). */
export interface TxOutcome {
  hash: string | null;
  url: string | null;
  ok: boolean;
  /** Contract panic message, or the RPC error text when there is no tx. */
  error: string | null;
  logs: string[];
  /** Decoded SuccessValue of the transaction, if any. */
  value: unknown;
  signer: string;
  receiver: string;
  method: string;
}

interface RpcTxLike {
  status?: { SuccessValue?: string; Failure?: unknown } | string;
  transaction?: { hash?: string };
  transaction_outcome?: { id?: string; outcome?: { logs?: string[]; status?: unknown } };
  receipts_outcome?: Array<{ outcome?: { logs?: string[]; status?: unknown } }>;
}

export function panicText(failure: unknown): string {
  const s = JSON.stringify(failure);
  const m = s.match(/Smart contract panicked: ([^"\\]*)/);
  if (m) return m[1];
  const e = s.match(/ExecutionError\\?":\\?"([^"\\]*)/);
  if (e) return e[1];
  return s.slice(0, 400);
}

export function decodeOutcome(
  r: RpcTxLike,
  signer: string,
  receiver: string,
  method: string,
): TxOutcome {
  const hash = r.transaction?.hash ?? r.transaction_outcome?.id ?? null;
  const logs: string[] = [];
  for (const o of r.receipts_outcome ?? []) logs.push(...(o.outcome?.logs ?? []));
  let ok = false;
  let error: string | null = null;
  let value: unknown = undefined;
  const st = r.status;
  if (st && typeof st === "object" && "SuccessValue" in st) {
    ok = true;
    const raw = st.SuccessValue ?? "";
    if (raw) {
      const text = Buffer.from(raw, "base64").toString("utf8");
      try {
        value = JSON.parse(text);
      } catch {
        value = text;
      }
    }
  } else if (st && typeof st === "object" && "Failure" in st) {
    error = panicText(st.Failure);
  } else {
    error = `unexpected status ${JSON.stringify(st)}`;
  }
  return {
    hash,
    url: hash ? explorerTx(hash) : null,
    ok,
    error,
    logs,
    value,
    signer,
    receiver,
    method,
  };
}

export interface CallOpts {
  deposit?: bigint;
  gas?: bigint;
}

/** Send one FunctionCall and wait for FINAL. Never throws for an on-chain
 *  failure; returns ok=false with the panic message and the tx hash. Throws
 *  only if the transaction could not be submitted at all (no hash), and then
 *  the caller decides; see `tryCall`. */
export async function call(
  account: Account,
  contractId: string,
  method: string,
  args: Record<string, unknown>,
  opts: CallOpts = {},
): Promise<TxOutcome> {
  const r = (await account.signAndSendTransaction({
    receiverId: contractId,
    actions: [actions.functionCall(method, args, opts.gas ?? 100n * TGAS, opts.deposit ?? 0n)],
    waitUntil: "FINAL",
    throwOnFailure: false,
  })) as unknown as RpcTxLike;
  return decodeOutcome(r, account.accountId, contractId, method);
}

/** Like `call`, but a submission-level refusal (no transaction created) is
 *  returned as ok=false, hash=null with the RPC error text. */
export async function tryCall(
  account: Account,
  contractId: string,
  method: string,
  args: Record<string, unknown>,
  opts: CallOpts = {},
): Promise<TxOutcome> {
  try {
    return await call(account, contractId, method, args, opts);
  } catch (e) {
    return {
      hash: null,
      url: null,
      ok: false,
      error: errorText(e),
      logs: [],
      value: undefined,
      signer: account.accountId,
      receiver: contractId,
      method,
    };
  }
}

export function errorText(e: unknown): string {
  if (e instanceof Error) {
    const name =
      e.constructor?.name && e.constructor.name !== "Error" ? `${e.constructor.name}: ` : "";
    return `${name}${e.message}`.slice(0, 500);
  }
  return String(e).slice(0, 500);
}

/** Final block timestamp in seconds (chain time, not wall clock). */
export async function chainNow(provider: Provider): Promise<number> {
  const b = await withRetry(() => provider.viewBlock({ finality: "final" } as never));
  return Math.floor(
    Number(
      BigInt((b as { header: { timestamp_nanosec: string } }).header.timestamp_nanosec) /
        1_000_000_000n,
    ),
  );
}

/** `view_account.code_hash` of `accountId` ("111...1" when no contract). */
export async function codeHashOf(provider: Provider, accountId: string): Promise<string> {
  const acc = (await withRetry(() =>
    provider.viewAccount({ accountId, blockQuery: { finality: "final" } } as never),
  )) as unknown as { code_hash: string };
  return acc.code_hash;
}
