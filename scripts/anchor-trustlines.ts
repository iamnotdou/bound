// Open trustlines to the SEP-24 reference anchor's USDC on every demo account.
//
// Why this exists, and why it is not part of `pnpm setup`:
//
// `setup-accounts.ts` deploys a *self-issued* test USDC where the operator is
// the issuer — and an issuer holds no trustline to its own asset, so only the
// four non-issuer accounts were ever trustlined. The anchor deployment flips
// that: the asset is issued by the anchor (GBBD47IF…), so the operator is an
// ordinary holder and needs a trustline like everyone else. It also receives
// the protocol fee share, because `.env.anchor` omits TREASURY_ADDRESS and
// deploy-all.ts falls back to the operator.
//
// Classic assets cannot be received without a trustline, and the anchor
// reports `claimable_balances: false`, so it will not route around a missing
// one. No trustline means the SEP-24 deposit simply fails.
//
//   BOUND_ENV_FILE=.env.anchor ts-node --project scripts/tsconfig.json scripts/anchor-trustlines.ts
import { readEnv, changeTrust } from "./lib";

const ANCHOR_USDC = "USDC:GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
const HORIZON = "https://horizon-testnet.stellar.org";
const ROLES = ["OPERATOR", "AGENT", "AUDITOR", "CHALLENGER", "COUNTERPARTY"] as const;

const [code, issuer] = ANCHOR_USDC.split(":");

async function hasTrustline(address: string): Promise<boolean> {
  const res = await fetch(`${HORIZON}/accounts/${address}`);
  if (!res.ok) throw new Error(`horizon ${res.status} for ${address}`);
  const body = (await res.json()) as { balances: Record<string, string>[] };
  return body.balances.some((b) => b.asset_code === code && b.asset_issuer === issuer);
}

async function main() {
  const env = readEnv();
  console.log(`Opening trustlines to ${ANCHOR_USDC}\n`);

  for (const role of ROLES) {
    const address = env[`${role}_ADDRESS`];
    const secret = env[`${role}_SECRET`];
    if (!address || !secret) throw new Error(`missing ${role}_ADDRESS/${role}_SECRET`);

    if (await hasTrustline(address)) {
      console.log(`  ${role.toLowerCase().padEnd(13)} already trustlined, skipping`);
      continue;
    }
    console.log(`  ${role.toLowerCase().padEnd(13)} opening…`);
    changeTrust(ANCHOR_USDC, secret);
    if (!(await hasTrustline(address))) throw new Error(`${role}: trustline did not appear`);
    console.log(`  ${role.toLowerCase().padEnd(13)} ok`);
  }

  console.log(`\n✓ All ${ROLES.length} accounts can now hold the anchor's USDC.`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
