// Read-only smoke test: prove the SDK + generated bindings talk to testnet.
import { bound } from "../app/lib/bound-client";
import { accounts } from "../app/lib/accounts";
import { formatUsdc } from "../app/lib/config";

async function main() {
  const agent = accounts.agent.publicKey();
  console.log("verifyCertificate(agent):");
  const v = await bound.verifyCertificate(agent);
  console.log(
    `  valid=${v.valid} status=${v.status.tag} bound=${formatUsdc(v.bound)} reserve=${formatUsdc(v.reserve)} auditorStake=${formatUsdc(v.auditor_stake)}`,
  );

  const cp = accounts.counterparty.publicKey();
  console.log(`usdcBalance(counterparty): ${formatUsdc(await bound.usdcBalance(cp))}`);
  console.log(`reserveBalance(): ${formatUsdc(await bound.reserveBalance())}`);
  console.log("\n✓ SDK reads live testnet through the generated bindings.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
