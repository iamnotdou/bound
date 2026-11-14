// Zero-key reads a counterparty makes before dealing with a NEAR agent:
// its Bound certificate (the contract's verdict) and its confinement (access
// keys plus contract code, which the contract cannot see). Testnet defaults
// come from the bundled deployment; pass `contractId` / `rpc` to override.

import type { Provider } from "near-api-js";
import { accountCodeOf, testnetProvider, view, withRetry } from "./client.js";
import {
  assess,
  coverageOf,
  globalContractOf,
  hasNoCode,
  isConfined,
  keysConfined,
  NO_CODE_HASH,
  type AccessKeyInfo,
  type AccountCode,
  type CertViewJson,
} from "./format.js";
import { TESTNET } from "./testnet.js";

export interface ReadOptions {
  /** A ready provider. Wins over `rpc`. */
  provider?: Provider;
  /** RPC URLs, tried in order. Default: the bundled testnet endpoints. */
  rpc?: string[];
  /** Bound contract account. Default: the bundled testnet deployment. */
  contractId?: string;
}

function resolve(opts: ReadOptions): { provider: Provider; contractId: string } {
  return {
    provider: opts.provider ?? testnetProvider(opts.rpc?.length ? opts.rpc : TESTNET.rpc),
    contractId: opts.contractId ?? TESTNET.contracts.bound.accountId,
  };
}

/** `verify(agent)` on the Bound contract: the agent's current certificate,
 *  or null when it has none. */
export async function getAgentCertificate(
  agent: string,
  opts: ReadOptions = {},
): Promise<CertViewJson | null> {
  const { provider, contractId } = resolve(opts);
  return (await view<CertViewJson | null>(provider, contractId, "verify", { agent })) ?? null;
}

/** `get_cert(cert_id)`: one certificate by id, or null. */
export async function getCertificate(
  certId: number,
  opts: ReadOptions = {},
): Promise<CertViewJson | null> {
  const { provider, contractId } = resolve(opts);
  return (
    (await view<CertViewJson | null>(provider, contractId, "get_cert", { cert_id: certId })) ?? null
  );
}

export interface Confinement {
  agent: string;
  contract: string;
  /** Every key a pay-only function-call key for `contract` AND no code on
   *  the account, local or global. */
  confined: boolean;
  keysConfined: boolean;
  noContractCode: boolean;
  noLocalCode: boolean;
  globalContract: string | null;
  code: AccountCode;
  keys: AccessKeyInfo[];
}

/** Reads the agent's access keys and code (local and NEAR global contract)
 *  and applies `isConfined`. */
export async function checkConfinement(
  agent: string,
  opts: ReadOptions = {},
): Promise<Confinement> {
  const { provider, contractId } = resolve(opts);
  const r = (await withRetry(() =>
    provider.viewAccessKeyList({ accountId: agent, finalityQuery: { finality: "final" } } as never),
  )) as unknown as { keys: AccessKeyInfo[] };
  const code = await accountCodeOf(provider, agent);
  return {
    agent,
    contract: contractId,
    confined: isConfined(r.keys, contractId, code),
    keysConfined: keysConfined(r.keys, contractId),
    noContractCode: hasNoCode(code),
    noLocalCode: code.code_hash === NO_CODE_HASH,
    globalContract: globalContractOf(code),
    code,
    keys: r.keys,
  };
}

export interface AgentVerdict {
  agent: string;
  contract: string;
  /** Deal only when true: Verified, not halted, float > 0. */
  deal: boolean;
  reason: string;
  certificate: CertViewJson | null;
  /** reserve_locked + auditor allocation, USDC base units (0 without a cert). */
  coverage: bigint;
  /** Null when the account does not exist or could not be read. */
  confinement: Confinement | null;
}

/** Everything a counterparty reads before dealing with `agent`: the
 *  contract's verdict, coverage and the agent's confinement. */
export async function verifyAgent(agent: string, opts: ReadOptions = {}): Promise<AgentVerdict> {
  const r = resolve(opts);
  const certificate = await getAgentCertificate(agent, r);
  const a = assess(certificate);
  let confinement: Confinement | null = null;
  try {
    confinement = await checkConfinement(agent, r);
  } catch {
    confinement = null;
  }
  return {
    agent,
    contract: r.contractId,
    deal: a.ok,
    reason: a.reason,
    certificate,
    coverage: certificate ? coverageOf(certificate) : 0n,
    confinement,
  };
}
