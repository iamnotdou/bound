// Exercise the agent tools' execute() directly — no Claude, no API key needed.
// Proves the tool layer is wired to BoundClient against live testnet.
// Read-only tools only (verify, balance) so we don't mutate state here.
import { boundTools } from "../app/lib/agent-tools";
import { accounts } from "../app/lib/accounts";

async function main() {
  const agent = accounts.agent.publicKey();

  console.log("verify_agent_certificate:");
  console.log(" ", JSON.stringify(await boundTools.verify_agent_certificate.execute({ agent })));

  console.log("get_balance (agent):");
  console.log(" ", JSON.stringify(await boundTools.get_balance.execute({})));

  console.log("\n✓ Tool layer is wired to BoundClient. (execute_payment / fetch_paid_service / challenge_certificate mutate state — exercised via the demo.)");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
