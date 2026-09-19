// SEP-10 authenticate against the reference anchor, then open a SEP-24
// interactive deposit so a demo account can be funded with the anchor's USDC.
//
// Phase 1 needs anchor-issued USDC in the accounts before Bound can be deployed
// against it, and the only source of that asset is the anchor itself. This is
// also the cheapest possible proof that SEP-10 works, which everything in the
// Phase 2 rail is built on top of.
//
//   BOUND_ENV_FILE=.env.anchor pnpm anchor-deposit            # operator
//   BOUND_ENV_FILE=.env.anchor pnpm anchor-deposit AUDITOR    # any role
//
// Prints an interactive URL. Open it, complete the anchor's test form, and the
// anchor sends USDC to the account. Check progress with `pnpm anchor-status <id>`.
import { Keypair, TransactionBuilder } from "@stellar/stellar-sdk";
import { readEnv } from "./lib";

const HOME_DOMAIN = "testanchor.stellar.org";
const AUTH = `https://${HOME_DOMAIN}/auth`;
const SEP24 = `https://${HOME_DOMAIN}/sep24`;
const ASSET_CODE = "USDC";

/** SEP-10: fetch a challenge, sign it, exchange it for a JWT. */
export async function sep10(secret: string): Promise<string> {
  const kp = Keypair.fromSecret(secret);

  const res = await fetch(`${AUTH}?account=${kp.publicKey()}&home_domain=${HOME_DOMAIN}`);
  if (!res.ok) throw new Error(`SEP-10 challenge failed: ${res.status} ${await res.text()}`);
  const { transaction, network_passphrase } = (await res.json()) as {
    transaction: string;
    network_passphrase: string;
  };

  const tx = TransactionBuilder.fromXDR(transaction, network_passphrase);
  tx.sign(kp);

  const token = await fetch(AUTH, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ transaction: tx.toXDR() }),
  });
  if (!token.ok) throw new Error(`SEP-10 token failed: ${token.status} ${await token.text()}`);
  const body = (await token.json()) as { token: string };
  return body.token;
}

async function main() {
  const role = (process.argv[2] ?? "OPERATOR").toUpperCase();
  const env = readEnv();
  const secret = env[`${role}_SECRET`];
  const address = env[`${role}_ADDRESS`];
  if (!secret || !address) throw new Error(`missing ${role}_SECRET/${role}_ADDRESS`);

  console.log(`Role:    ${role.toLowerCase()}`);
  console.log(`Account: ${address}\n`);

  console.log("SEP-10 authenticating…");
  const jwt = await sep10(secret);
  console.log(`  ok, JWT received (${jwt.length} chars)\n`);

  console.log("SEP-24 requesting interactive deposit…");
  const res = await fetch(`${SEP24}/transactions/deposit/interactive`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${jwt}` },
    body: JSON.stringify({ asset_code: ASSET_CODE, account: address }),
  });
  if (!res.ok) throw new Error(`SEP-24 deposit failed: ${res.status} ${await res.text()}`);
  const { id, url } = (await res.json()) as { id: string; url: string };

  console.log(`  transaction id: ${id}\n`);
  console.log("Open this and complete the anchor's form:\n");
  console.log(`  ${url}\n`);
  console.log(`Then: BOUND_ENV_FILE=.env.anchor pnpm anchor-status ${id}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
