// Marketplace discovery — enumerate published certificates.
//
// SERVER ONLY, like config.ts and tx.ts: it reaches the chain over RPC.
// Read-only simulations, no signer, no secrets.
//
// WHY THIS IS DRIVEN BY CERT ID, NEVER BY AGENT ADDRESS:
// Registry::publish authenticates only the *operator* and then overwrites the
// `AgentCert(agent)` mapping unconditionally — any operator can repoint any
// agent at a certificate of their choosing. That mapping (and therefore
// `verify(agent)` / `get_cert_id(agent)`, which both read it) is not
// trustworthy for listing. The certificate records themselves are keyed by an
// append-only id (`CertCount` → `Certificate(id)`), so the listing walks
// `get_cert_count()` and `get_certificate(cert_id)` and takes the agent FROM
// the certificate, not the other way round.
import { Client as RegistryClient, type Certificate } from "../../../bindings/registry/src";

import { contracts, network, readSource } from "./config";
import { toCertView, type CertView } from "./cert-view";
import type { VerifyResult } from "./bound-client";

export interface CertListItem extends CertView {
  certId: number;
}

const DEFAULT_LIMIT = 50;
const FETCH_CONCURRENCY = 8;

function registry(): RegistryClient {
  return new RegistryClient({
    contractId: contracts.registry,
    networkPassphrase: network.passphrase,
    rpcUrl: network.rpcUrl,
    publicKey: readSource,
  });
}

/**
 * Project a stored Certificate into the same VerifyResult shape the registry's
 * `verify` returns, so the listing can reuse `toCertView` rather than growing a
 * second projection. `verify` computes `valid` against the ledger timestamp; we
 * have no ledger here, so expiry is checked against the local clock.
 */
function asVerifyResult(cert: Certificate): VerifyResult {
  const expired = BigInt(Math.floor(Date.now() / 1000)) > BigInt(cert.expires_at);
  return {
    valid: cert.status.tag === "Verified" && !expired,
    status: cert.status,
    bound: cert.bound,
    reserve: cert.reserve_amount,
    auditor_stake: cert.auditor_stake_snapshot,
    auditor: cert.auditor,
    expires_at: cert.expires_at,
  } as VerifyResult;
}

/**
 * List published certificates, newest first (highest cert id first).
 *
 * A certificate that fails to load individually is skipped — one bad record
 * must not take the whole listing down.
 */
export async function listCertificates(opts?: {
  limit?: number;
  offset?: number;
}): Promise<CertListItem[]> {
  const limit = Math.max(0, Math.trunc(opts?.limit ?? DEFAULT_LIMIT));
  const offset = Math.max(0, Math.trunc(opts?.offset ?? 0));
  if (limit === 0) return [];

  const client = registry();
  const count = Number((await client.get_cert_count()).result);
  if (!Number.isFinite(count) || count <= 0) return [];

  // Ids are 1-based and append-only, so newest-first is simply descending id.
  const ids: number[] = [];
  for (let id = count - offset; id >= 1 && ids.length < limit; id--) ids.push(id);

  // Fetched in bounded-concurrency batches: one RPC round-trip per certificate,
  // so a serial loop would make a 50-row page 50 round-trips deep. The cap keeps
  // us from opening fifty sockets at a public RPC endpoint at once.
  const items: CertListItem[] = [];
  for (let i = 0; i < ids.length; i += FETCH_CONCURRENCY) {
    const batch = await Promise.all(
      ids.slice(i, i + FETCH_CONCURRENCY).map(async (certId) => {
        try {
          const cert = (await client.get_certificate({ cert_id: BigInt(certId) })).result;
          return { ...toCertView(cert.agent, asVerifyResult(cert), certId), certId };
        } catch {
          // certificate_not_found, or a record this SDK version cannot decode.
          return null;
        }
      }),
    );
    for (const item of batch) if (item) items.push(item);
  }
  return items;
}
