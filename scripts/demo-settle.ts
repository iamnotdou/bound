// Bound Protocol — act 7: close the claim window and settle.
//
//   pnpm run demo:settle
//
// Reads .demo-run.json, written by `pnpm run demo`. Run it once the window that
// run opened has lapsed; the script says how long is left if it has not.
//
// `close_window` is permissionless and pays its caller nothing. That looks like
// a missing incentive and is not one: no claimant is paid until somebody calls
// it, so every claimant has a reason to, and none of them can be favoured by
// calling it first. A fee here would just be a race with a prize attached.
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Keypair } from "@stellar/stellar-sdk";
import { bound, formatUsdc } from "@bound/sdk";
import { readEnv } from "./lib";

const STATE_PATH = resolve(__dirname, "..", ".demo-run.json");

const env = readEnv();
const need = (k: string): string => {
  const v = env[k];
  if (!v) throw new Error(`missing ${k} in .env.testnet`);
  return v;
};

const auditor = Keypair.fromSecret(need("AUDITOR_SECRET"));
const challenger = Keypair.fromSecret(need("CHALLENGER_SECRET"));

const ok = (line: string) => console.log(`  \x1b[32m✓\x1b[0m ${line}`);
const note = (line: string) => console.log(`    ${line}`);
const money = (stroops: bigint) => `${formatUsdc(stroops)} (${stroops.toLocaleString()} stroops)`;

interface RunState {
  certId: number;
  challengeId: number;
  agent: string;
  windowClosesAt: number;
  premiumStroops: string;
}

async function main() {
  if (!existsSync(STATE_PATH)) {
    throw new Error(`no .demo-run.json — run \`pnpm run demo\` first`);
  }
  const state = JSON.parse(readFileSync(STATE_PATH, "utf8")) as RunState;
  const certId = BigInt(state.certId);

  console.log("═".repeat(62));
  console.log(`  Bound Protocol — settling certificate #${state.certId}`);
  console.log("═".repeat(62));

  if (await bound.claimWindowSettled(certId)) {
    ok("this certificate's window is already closed and settled");
  } else {
    const closesAt = Number(await bound.windowClosesAt(certId));
    const now = Math.floor(Date.now() / 1000);
    if (now < closesAt) {
      const left = closesAt - now;
      const hours = Math.floor(left / 3600);
      const minutes = Math.round((left % 3600) / 60);
      console.log(`\n  The window is still open. ${hours}h ${minutes}m left.`);
      console.log(`  It may be closed at ${new Date(closesAt * 1000).toISOString()}.\n`);
      console.log("  This wait is the protocol working, not the demo stalling: a claim");
      console.log("  filed in hour 71 is admitted on the same terms as one filed in hour 1.\n");
      process.exit(0);
    }

    // Anyone may call this. The challenger does here only because they are the
    // party in this story with a reason to be watching the clock.
    await bound.closeClaimWindow(challenger, certId);
    ok("window closed — every admitted claim settled in one transaction");
  }

  // ---------------------------------------------------------------------------
  console.log("\n\x1b[1mWhat the settlement did\x1b[0m");
  // ---------------------------------------------------------------------------
  const verdict = await bound.verifyCertificate(state.agent);
  ok(`certificate is now ${verdict.status.tag}, valid: ${verdict.valid}`);
  note("BoundExceeded proves a covenant broke, not that money was lost. With no");
  note("assessed harm the settlement runs in hygiene mode: the certificate is");
  note("killed, the challenger is paid a flat bounty for the service of ending it,");
  note("and the auditor's stake is NOT slashed. A counter is never a loss, and a");
  note("protocol that slashed on one would be paying people to manufacture them.");

  // ---------------------------------------------------------------------------
  console.log("\n\x1b[1mThe auditor's side of the trade\x1b[0m");
  // ---------------------------------------------------------------------------
  const accrued = await bound.premiumAccrued(certId);
  const claimable = await bound.premiumClaimable(certId);
  console.log(`  premium paid by the operator: ${money(BigInt(state.premiumStroops))}`);
  console.log(`  accrued to the auditor:       ${money(accrued)}`);
  console.log(`  claimable right now:          ${money(claimable)}`);

  if (claimable > 0n) {
    const { result: claimed } = await bound.claimPremium(auditor, certId);
    ok(`auditor withdrew ${money(claimed)} of yield on their staked capital`);
  } else {
    ok("nothing left to claim");
  }

  const stake = await bound.auditorStake(auditor.publicKey());
  console.log(`  auditor stake still locked:   ${formatUsdc(stake)}`);

  console.log("\n" + "═".repeat(62));
  console.log("  Lifecycle complete: issued → insured → routed → metered →");
  console.log("  proven → settled → yield claimed.");
  console.log("═".repeat(62));
  console.log(`
  The auditor made money for being right and would have lost their stake for
  being wrong. That is the whole business: the guarantee costs the operator a
  premium, pays the auditor a yield, and is only worth anything because the
  auditor has something to lose when it turns out to be a lie.
`);
}

main().catch((err) => {
  console.error(`\n\x1b[31m✗ ${err?.message ?? err}\x1b[0m`);
  process.exit(1);
});
