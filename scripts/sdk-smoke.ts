// Read-only smoke test: prove the SDK + generated bindings talk to testnet.
import { bound, formatUsdc } from "@bound/sdk";
import { readEnv } from "./lib";

const env = readEnv();

async function main() {
  const agent = env.AGENT_ADDRESS;
  console.log("verifyCertificate(agent):");
  const v = await bound.verifyCertificate(agent);
  console.log(
    `  valid=${v.valid} status=${v.status.tag} bound=${formatUsdc(v.bound)} reserve=${formatUsdc(v.reserve)} auditorStake=${formatUsdc(v.auditor_stake)}`,
  );

  const cp = env.COUNTERPARTY_ADDRESS;
  console.log(`usdcBalance(counterparty): ${formatUsdc(await bound.usdcBalance(cp))}`);
  const certId = await bound.certIdForAgent(agent);
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
