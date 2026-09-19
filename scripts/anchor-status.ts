// Poll a SEP-24 transaction to completion and report the on-chain result.
//
//   BOUND_ENV_FILE=.env.anchor pnpm anchor-status <transaction-id> [ROLE]
import { readEnv } from "./lib";
import { sep10 } from "./anchor-deposit";

const HOME_DOMAIN = "testanchor.stellar.org";
const SEP24 = `https://${HOME_DOMAIN}/sep24`;
const ANCHOR_ISSUER = "GBBD47IF6LWK7P7MDEVSCWR7DPUWV3NY3DTQEVFL4NAT4AQH3ZLLFLA5";
const HORIZON = "https://horizon-testnet.stellar.org";

async function anchorBalance(address: string): Promise<string> {
  const res = await fetch(`${HORIZON}/accounts/${address}`);
  const body = (await res.json()) as { balances: Record<string, string>[] };
  const line = body.balances.find(
    (b) => b.asset_code === "USDC" && b.asset_issuer === ANCHOR_ISSUER,
  );
  return line ? line.balance : "no trustline";
}

async function main() {
  const id = process.argv[2];
  if (!id) throw new Error("usage: pnpm anchor-status <transaction-id> [ROLE]");
  const role = (process.argv[3] ?? "OPERATOR").toUpperCase();

  const env = readEnv();
  const secret = env[`${role}_SECRET`];
  const address = env[`${role}_ADDRESS`];
  if (!secret || !address) throw new Error(`missing ${role}_SECRET/${role}_ADDRESS`);

  const jwt = await sep10(secret);
  const res = await fetch(`${SEP24}/transaction?id=${id}`, {
    headers: { Authorization: `Bearer ${jwt}` },
  });
  if (!res.ok) throw new Error(`status failed: ${res.status} ${await res.text()}`);
  const { transaction: t } = (await res.json()) as { transaction: Record<string, string> };

  console.log(`  id:             ${t.id}`);
  console.log(`  kind:           ${t.kind}`);
  console.log(`  status:         ${t.status}`);
  console.log(`  amount_in:      ${t.amount_in ?? "-"}`);
  console.log(`  amount_out:     ${t.amount_out ?? "-"}`);
  console.log(`  stellar tx:     ${t.stellar_transaction_id ?? "-"}`);
  console.log(`\n  ${role.toLowerCase()} anchor USDC balance: ${await anchorBalance(address)}`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
