// Read-only smoke test: prove the SDK + generated bindings talk to testnet.
import { bound, formatUsdc } from "@bound/sdk";
import { accounts } from "../apps/dashboard/app/lib/accounts";

async function main() {
  const agent = accounts.agent.publicKey();
  console.log("verifyCertificate(agent):");
  const v = await bound.verifyCertificate(agent);
  console.log(
    `  valid=${v.valid} status=${v.status.tag} bound=${formatUsdc(v.bound)} reserve=${formatUsdc(v.reserve)} auditorStake=${formatUsdc(v.auditor_stake)}`,
  );

  const cp = accounts.counterparty.publicKey();
  console.log(`usdcBalance(counterparty): ${formatUsdc(await bound.usdcBalance(cp))}`);
  const certId = await bound.certIdForAgent(accounts.agent.publicKey());
  console.log(
    `reserveBalance(cert ${certId}): ${
      certId === null ? "no certificate" : formatUsdc(await bound.reserveBalance(BigInt(certId)))
    }`,
  );
  console.log("\n✓ SDK reads live testnet through the generated bindings.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
