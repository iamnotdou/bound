// A JSON-safe projection of a Bound Certificate for any client (UI, HTTP).
// The on-chain VerifyResult carries i128 (bigint) fields that can't cross a
// JSON boundary, so API routes convert to this shape server-side. The TYPE is
// safe to `import type` into client code; the formatter pulls in server-only
// config and must only run on the server.
import { formatUsdc } from "./config";
import type { VerifyResult } from "./bound-client";

export type CertStatusTag = "Pending" | "Verified" | "Invalid";

export interface CertView {
  agent: string;
  /** Verified AND not expired — the only state a counterparty should accept. */
  valid: boolean;
  status: CertStatusTag;
  boundUsd: string;
  reserveUsd: string;
  auditorStakeUsd: string;
  auditor: string | null;
  expiresAtUnix: number;
  expiresAtIso: string | null;
  /** false when the registry has no certificate mapped to this agent at all. */
  hasCert: boolean;
}

export function toCertView(agent: string, v: VerifyResult): CertView {
  const hasCert = v.bound > 0n || v.auditor != null;
  const expiresAtUnix = Number(v.expires_at);
  return {
    agent,
    valid: v.valid,
    status: v.status.tag as CertStatusTag,
    boundUsd: formatUsdc(v.bound),
    reserveUsd: formatUsdc(v.reserve),
    auditorStakeUsd: formatUsdc(v.auditor_stake),
    auditor: v.auditor ?? null,
    expiresAtUnix,
    expiresAtIso: expiresAtUnix > 0 ? new Date(expiresAtUnix * 1000).toISOString() : null,
    hasCert,
  };
}
