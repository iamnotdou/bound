// Generate + fund the 5 demo accounts, then deploy a self-mintable test USDC
// and seed every account with it.
//
//   pnpm setup   (→ ts-node --project scripts/tsconfig.json scripts/setup-accounts.ts)
//
// Steps:
//   1. Generate any missing keypairs, fund XLM via Friendbot (fees + account creation).
//   2. Deploy a Stellar Asset Contract for USDC issued by the operator → USDC_ADDRESS.
//   3. Open a trustline for each non-issuer account and mint it test USDC.
//
// The operator is the issuer, so it can fund the reserve by "issuing" straight
// into the vault contract (no trustline needed on contract balances). Re-running
// is idempotent: the SAC id is deterministic and mint just tops balances up.
//
// IMPORTANT: this overwrites USDC_ADDRESS with our test SAC, so `pnpm deploy`
// must be (re-)run afterwards — the contracts bind their token at initialize().
import { Keypair } from "@stellar/stellar-sdk";
import { readEnv, writeEnvValues, deployAssetSac, changeTrust, mint, usdc } from "./lib";

const ROLES = ["OPERATOR", "AGENT", "AUDITOR", "CHALLENGER", "COUNTERPARTY"] as const;
const ISSUER_ROLE = "OPERATOR";
const MINT_AMOUNT = usdc(100_000); // $100k each — plenty for any demo scenario
const FRIENDBOT = "https://friendbot.stellar.org";

async function fund(publicKey: string): Promise<void> {
  const res = await fetch(`${FRIENDBOT}/?addr=${encodeURIComponent(publicKey)}`);
  if (res.ok) return;
  const body = await res.text();
  if (/op_already_exists|already.*funded|createAccountAlreadyExist/i.test(body)) {
    console.log(`    (already funded)`);
    return;
  }
  throw new Error(`friendbot failed for ${publicKey}: ${res.status} ${body}`);
}

async function main() {
  const env = readEnv();
  const updates: Record<string, string> = {};
  const kp: Record<string, Keypair> = {};

  // --- 1. accounts + XLM ---
  for (const role of ROLES) {
    const secretKey = `${role}_SECRET`;
    const addrKey = `${role}_ADDRESS`;

    if (env[secretKey]) {
      kp[role] = Keypair.fromSecret(env[secretKey]);
      console.log(`${role}: reusing ${kp[role].publicKey()}`);
    } else {
      kp[role] = Keypair.random();
      updates[secretKey] = kp[role].secret();
      console.log(`${role}: generated ${kp[role].publicKey()}`);
    }
    if (env[addrKey] !== kp[role].publicKey()) updates[addrKey] = kp[role].publicKey();

    console.log(`  funding via friendbot…`);
    await fund(kp[role].publicKey());
  }

  if (Object.keys(updates).length > 0) writeEnvValues(updates);

  // --- 2. deploy test USDC SAC (issuer = operator) ---
  const issuer = kp[ISSUER_ROLE];
  const asset = `USDC:${issuer.publicKey()}`;
  console.log(`\nDeploying test USDC asset contract (${asset})…`);
  const usdcSac = deployAssetSac(asset, issuer.secret());
  console.log(`  USDC_ADDRESS = ${usdcSac}`);
  writeEnvValues({ USDC_ADDRESS: usdcSac });

  // --- 3. trustlines + mint for every non-issuer account ---
  console.log(`\nSeeding accounts with $100k test USDC each…`);
  for (const role of ROLES) {
    if (role === ISSUER_ROLE) continue; // issuer needs no trustline / mint
    console.log(`  ${role}: trustline + mint`);
    changeTrust(asset, kp[role].secret());
    mint(usdcSac, issuer.secret(), kp[role].publicKey(), MINT_AMOUNT);
  }

  console.log(`\n✓ Accounts funded and seeded with test USDC.`);
  console.log(`\n⚠  USDC_ADDRESS changed — run \`pnpm deploy\` so contracts bind to the new token.`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
