// Pure helpers: amounts, explorer links, NEP-297 event parsing, cert views.
// No network, so they are unit tested with vitest.

export const USDC_DECIMALS = 6;
export const USDC = 10n ** 6n;

export const explorerTx = (hash: string) => `https://testnet.nearblocks.io/txns/${hash}`;
export const explorerAccount = (id: string) => `https://testnet.nearblocks.io/address/${id}`;

/** "12.5" -> 12500000n (USDC base units). */
export function usdc(amount: string | number): bigint {
  const s = String(amount).trim();
  if (!/^\d+(\.\d{1,6})?$/.test(s)) throw new Error(`invalid USDC amount: ${s}`);
  const [whole, frac = ""] = s.split(".");
  return BigInt(whole) * USDC + BigInt(frac.padEnd(USDC_DECIMALS, "0"));
}

/** 12500000n -> "12.5" */
export function fmtUsdc(base: bigint | string): string {
  const v = BigInt(base);
  const neg = v < 0n;
  const a = neg ? -v : v;
  const whole = a / USDC;
  const frac = (a % USDC).toString().padStart(USDC_DECIMALS, "0").replace(/0+$/, "");
  return `${neg ? "-" : ""}${whole}${frac ? `.${frac}` : ""}`;
}

export interface BoundEvent {
  standard: string;
  version: string;
  event: string;
  data: Array<Record<string, unknown>>;
}

/** Parse every `EVENT_JSON:` log with standard "bound". */
export function parseEvents(logs: string[]): BoundEvent[] {
  const out: BoundEvent[] = [];
  for (const l of logs) {
    if (!l.startsWith("EVENT_JSON:")) continue;
    try {
      const e = JSON.parse(l.slice("EVENT_JSON:".length)) as BoundEvent;
      if (e.standard === "bound") out.push(e);
    } catch {
      // not ours
    }
  }
  return out;
}

export interface PayEvent {
  cert_id: number;
  agent: string;
  to: string;
  amount: bigint;
  memo: string;
}

export function payEvents(logs: string[]): PayEvent[] {
  return parseEvents(logs)
    .filter((e) => e.event === "pay")
    .flatMap((e) => e.data)
    .map((d) => ({
      cert_id: Number(d.cert_id),
      agent: String(d.agent),
      to: String(d.to),
      amount: BigInt(String(d.amount)),
      memo: String(d.memo ?? ""),
    }));
}

/** JSON shape returned by `verify` / `get_cert`. Amounts are decimal strings. */
export interface CertViewJson {
  cert_id: number;
  status: string;
  operator: string;
  agent: string;
  auditor: string;
  bound: string;
  reserve_promised: string;
  reserve_locked: string;
  allocation: string;
  /** reserve_locked + allocation: the most arbitrated victims can recover. */
  coverage?: string;
  premium: string;
  float: string;
  float_cap: string;
  spent: string;
  post_expiry_spent: string;
  pay_count: number;
  allowlist: string[] | null;
  expires_at: number;
  settlement_deadline: number;
  halted: boolean;
  window_closes_at: number | null;
  open_claims: number;
  settlement: Record<string, unknown> | null;
  now: number;
  /** The contract's own verdict: Verified, not halted and float > 0. */
  deal?: boolean;
}

/** A counterparty's decision from a cert view: deal only with "Verified". */
export function assess(view: CertViewJson | null | undefined): { ok: boolean; reason: string } {
  if (!view) return { ok: false, reason: "no Bound certificate for this agent" };
  if (view.status !== "Verified")
    return { ok: false, reason: `certificate status is ${view.status}` };
  if (view.halted) return { ok: false, reason: "certificate halted by its operator" };
  // Newer contracts also require float > 0; trust their verdict when present.
  if (view.deal === false) return { ok: false, reason: "certificate has no float to pay from" };
  return { ok: true, reason: "Verified" };
}

/** One-line human summary of a cert view. */
export function describeCert(v: CertViewJson): string {
  return [
    `cert ${v.cert_id} ${v.status}`,
    `agent ${v.agent}`,
    `bound ${fmtUsdc(v.bound)}`,
    `reserve ${fmtUsdc(v.reserve_locked)}/${fmtUsdc(v.reserve_promised)}`,
    `auditor ${v.auditor} allocation ${fmtUsdc(v.allocation)}`,
    `coverage ${fmtUsdc(coverageOf(v))}`,
    `spent ${fmtUsdc(v.spent)} float ${fmtUsdc(v.float)}/${fmtUsdc(v.float_cap)}`,
    `expires ${new Date(v.expires_at * 1000).toISOString()}`,
  ].join(" | ");
}

/** One entry of RPC `view_access_key_list`. */
export interface AccessKeyInfo {
  public_key: string;
  access_key: {
    nonce: number;
    permission:
      | "FullAccess"
      | {
          FunctionCall: {
            allowance: string | null;
            receiver_id: string;
            method_names: string[];
          };
        };
  };
}

/** `code_hash` of an account with no contract deployed. */
export const NO_CODE_HASH = "11111111111111111111111111111111";

/** What a counterparty can rely on: reserve_locked + allocation. */
export function coverageOf(v: CertViewJson): bigint {
  return v.coverage !== undefined
    ? BigInt(v.coverage)
    : BigInt(v.reserve_locked) + BigInt(v.allocation);
}

/** True when the keys alone are confined: every key is a FunctionCall key to
 *  `contractId` limited to `pay`. Not enough by itself: see `isConfined`. */
export function keysConfined(keys: AccessKeyInfo[], contractId: string): boolean {
  return (
    keys.length > 0 &&
    keys.every((k) => {
      const p = k.access_key.permission;
      return (
        p !== "FullAccess" &&
        p.FunctionCall.receiver_id === contractId &&
        p.FunctionCall.method_names.length > 0 &&
        p.FunctionCall.method_names.every((m) => m === "pay")
      );
    })
  );
}

/** The `view_account` fields confinement needs. An account that uses a NEAR
 *  global contract reports `code_hash` of all ones, like an account with no
 *  code, and names the global contract in `global_contract_hash` or
 *  `global_contract_account_id` instead (both absent or null otherwise).
 *  That code runs as the account just like local code. */
export interface AccountCode {
  code_hash: string;
  global_contract_hash?: string | null;
  global_contract_account_id?: string | null;
}

/** The global contract an account uses (its hash or the account that
 *  publishes it), or null when it uses none. */
export function globalContractOf(account: AccountCode | undefined): string | null {
  return account?.global_contract_hash || account?.global_contract_account_id || null;
}

/** True when the account runs no code at all: `code_hash` is all ones AND
 *  it uses no global contract. */
export function hasNoCode(account: AccountCode | undefined): boolean {
  return account?.code_hash === NO_CODE_HASH && globalContractOf(account) === null;
}

/** Confined = keys confined AND no code on the agent account: `code_hash`
 *  all ones and both `global_contract_*` fields empty. Code on the account
 *  (local or global) would run as the agent and could call `pay`,
 *  `ft_transfer` or add a full-access key without any key. The Bound
 *  contract cannot check this itself, so a counterparty reads
 *  `verify(agent)` AND both of these. */
export function isConfined(
  keys: AccessKeyInfo[],
  contractId: string,
  account: AccountCode | undefined,
): boolean {
  return hasNoCode(account) && keysConfined(keys, contractId);
}

/** One line on the account's code, for logs and the verify CLI. */
export function describeCode(account: AccountCode | undefined): string {
  if (!account) return "code unknown";
  const g = globalContractOf(account);
  if (g) return `global contract ${g}`;
  return account.code_hash === NO_CODE_HASH
    ? "no contract code, no global contract"
    : `contract code ${account.code_hash}`;
}
