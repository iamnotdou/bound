// Bound Protocol — the full v2 lifecycle, against live Stellar testnet.
//
//   pnpm run demo          — acts 1-6: issue, insure, route, meter, prove
//   pnpm run demo:settle   — act 7: close the claim window and pay out
//
// Two commands rather than one because the claim window is 72 hours and that
// is not a placeholder. A challenge does not settle when it is filed; it opens
// a window that every other claimant against the same certificate may join, and
// the whole set is priced together when the window lapses. v1 settled the first
// claim to arrive and foreclosed every honest one behind it. Collapsing the
// window to make a demo finish in one run would be demonstrating a protocol we
// deliberately do not ship.
//
// The story this run tells:
//
//   An operator publishes a certificate bounding an agent's counterparty losses
//   at $500, funds the reserve in full, and an auditor bonds their own slashable
//   capital behind it. The operator buys coverage; the premium starts accruing
//   to the auditor as yield. The agent is enrolled in the PaymentRouter and
//   every payment it makes is metered against the certificate.
//
//   Then the agent routes $600 through it. Nobody is defrauded and nothing is
//   stolen — but the operator promised a counterparty's exposure was capped at
//   $500, and the router's own counter now says otherwise. That is a covenant
//   broken, it is visible in on-chain state, and ANYONE can prove it by
//   arithmetic. No arbiter, no oracle, no referee.
//
// Everything here goes through @bound/sdk. That is the point: the SDK a third
// party installs is the same surface this demo drives, so a lifecycle this
// script cannot express is a lifecycle the SDK does not really cover.
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { Keypair } from "@stellar/stellar-sdk";
import { bound, contracts, formatUsdc, usdc } from "@bound/sdk";
import { readEnv, changeTrust } from "./lib";

const env = readEnv();
const need = (k: string): string => {
  const v = env[k];
  if (!v) throw new Error(`missing ${k} in .env.testnet — run \`pnpm setup\` && \`pnpm deploy\``);
  return v;
};

const operator = Keypair.fromSecret(need("OPERATOR_SECRET"));
const auditor = Keypair.fromSecret(need("AUDITOR_SECRET"));
const challenger = Keypair.fromSecret(need("CHALLENGER_SECRET"));
const counterparty = need("COUNTERPARTY_ADDRESS");

// A fresh agent every run, for a reason the router enforces rather than
// suggests: an enrollment is permanent. An operator may not walk an agent off a
// certificate whose spend counter is climbing and onto a clean one, because a
// counter you can escape is not evidence. So a second run needs a second agent,
// and the demo generates one rather than failing on `already_enrolled`.
const agent = Keypair.random();

// Amounts (USDC, 7 decimals). Small on purpose: every one of these is real
// testnet money moving through real contracts, and the numbers are chosen so a
// reader can do the arithmetic in their head.
const AUDITOR_STAKE = usdc(1_500);
const ALLOCATION = usdc(500); // the auditor's slice bonded to THIS certificate
const BOUND = usdc(500);
const RESERVE = usdc(500); // funded in full — this operator is not lying about money
const FLOAT_CAP = usdc(200); // the most a stolen agent key can reach at any moment
const PAYMENT = usdc(200);
const PAYMENTS = 3; // 3 x $200 = $600 > the $500 bound
// The ChallengeManager enforces a minimum bond, and it is not small. That is
// the point: a challenge is an accusation backed by the challenger's own money,
// and a bond you would not miss is not a deterrent to filing a false one.
const CHALLENGE_BOND = usdc(500);
const AGENT_FUNDING = usdc(1_000);

// A one-hour term. The premium is priced on bound x duration, so a short term
// keeps the demo honest about accrual: yield you can watch arrive in a minute
// is yield the contract is really paying, not a number printed for effect.
const TERM_SECONDS = 3_600;

const STATE_PATH = resolve(__dirname, "..", ".demo-run.json");
const FRIENDBOT = "https://friendbot.stellar.org";

let actNo = 0;
function act(title: string) {
  actNo += 1;
  console.log(`\n\x1b[1mAct ${actNo}/6  ${title}\x1b[0m`);
}
const ok = (line: string) => console.log(`  \x1b[32m✓\x1b[0m ${line}`);
const note = (line: string) => console.log(`    ${line}`);

/** Stroops read as dollars round to nothing at these sizes; show both. */
const money = (stroops: bigint) => `${formatUsdc(stroops)} (${stroops.toLocaleString()} stroops)`;

async function friendbotFund(publicKey: string): Promise<void> {
  const res = await fetch(`${FRIENDBOT}?addr=${encodeURIComponent(publicKey)}`);
  if (!res.ok) {
    throw new Error(`friendbot failed for ${publicKey}: ${res.status} ${await res.text()}`);
  }
}

async function main() {
  console.log("═".repeat(62));
  console.log("  Bound Protocol — full lifecycle (Stellar testnet)");
  console.log("═".repeat(62));

  if (!contracts.paymentRouter || !contracts.premiumVault) {
    throw new Error(
      "this deployment predates the router and the premium vault — run `pnpm deploy` first",
    );
  }

  // ---------------------------------------------------------------------------
  act("A new agent gets an identity and some money");
  // ---------------------------------------------------------------------------
  console.log(`  agent: ${agent.publicKey()}`);
  await friendbotFund(agent.publicKey());
  changeTrust(`USDC:${operator.publicKey()}`, agent.secret());
  await bound.mintUsdc(operator, agent.publicKey(), AGENT_FUNDING);
  ok(`funded with XLM and ${formatUsdc(AGENT_FUNDING)} test USDC`);

  // ---------------------------------------------------------------------------
  act("The operator issues a certificate, and an auditor stakes behind it");
  // ---------------------------------------------------------------------------
  await bound.stakeAsAuditor(auditor, AUDITOR_STAKE);
  ok(
    `auditor locked ${formatUsdc(await bound.auditorStake(auditor.publicKey()))} of their own capital`,
  );

  const expiresAt = BigInt(Math.floor(Date.now() / 1000) + TERM_SECONDS);
  const certId = await bound.publishCertificate(operator, agent, {
    bound: BOUND,
    reserveAmount: RESERVE,
    expiresAt,
  });
  ok(`certificate #${certId} published — bound ${formatUsdc(BOUND)}, term ${TERM_SECONDS / 3600}h`);
  note("the operator signed the transaction and the agent signed its own");
  note("authorization entry: nobody can be bonded without consenting to it");

  await bound.depositReserve(operator, certId, RESERVE);
  ok(
    `reserve funded: ${formatUsdc(await bound.reserveBalance(certId))} against a claim of ${formatUsdc(RESERVE)}`,
  );

  await bound.attestCertificate(auditor, certId, ALLOCATION);
  const verified = await bound.verifyCertificate(agent.publicKey());
  ok(
    `auditor attested ${formatUsdc(ALLOCATION)} — certificate is ${verified.status.tag}, valid: ${verified.valid}`,
  );

  // ---------------------------------------------------------------------------
  act("The operator buys coverage — the auditor starts earning");
  // ---------------------------------------------------------------------------
  const quote = await bound.quotePremiumForCert(certId);
  note(`premium = bound x rate x duration / 1 year`);
  note(`        = ${formatUsdc(BOUND)} x 2.00%/yr x ${TERM_SECONDS}s / 31,536,000s`);
  note(`        = ${money(quote)}`);
  await bound.payPremium(operator, certId);
  const coverage = await bound.coverage(certId);
  ok(`coverage bought: ${money(quote)}, of which the protocol keeps its fee share`);
  ok(`the rest accrues to the auditor in a straight line across the term`);
  note(`auditor of record: ${coverage.auditor}`);

  // ---------------------------------------------------------------------------
  act("The agent is enrolled in the router, and starts paying");
  // ---------------------------------------------------------------------------
  await bound.enrollAgent(operator, agent, certId, FLOAT_CAP);
  ok(`agent enrolled against certificate #${certId}, float cap ${formatUsdc(FLOAT_CAP)}`);
  note("both signatures again, and for symmetric reasons: enrollment attaches");
  note("spend to the operator's certificate and puts the agent's address under");
  note("the operator's kill switch. Neither party may conscript the other.");

  for (let i = 1; i <= PAYMENTS; i++) {
    const receipt = await bound.executePayment(agent, counterparty, PAYMENT);
    if (!receipt.routed) {
      throw new Error(
        "payment did not go through the router — the spend meter never moved, " +
          "so BoundExceeded would be unprovable. This is the bug the demo exists to catch.",
      );
    }
    const spent = await bound.spendForCert(certId);
    ok(
      `payment ${i}/${PAYMENTS}: ${formatUsdc(PAYMENT)} routed · ` +
        `metered spend now ${formatUsdc(spent)} of a ${formatUsdc(BOUND)} bound`,
    );
  }

  const spent = await bound.spendForCert(certId);
  console.log("");
  note(
    `float held right now: ${formatUsdc(await bound.floatForCert(certId))} (cap ${formatUsdc(FLOAT_CAP)})`,
  );
  note("float is topped up per payment rather than parked, so the cap bounds what");
  note("a stolen key reaches without ever making the agent unable to pay.");

  // ---------------------------------------------------------------------------
  act("Anyone can now prove the covenant broke — by arithmetic");
  // ---------------------------------------------------------------------------
  console.log(`  the certificate says:  bound        = ${formatUsdc(BOUND)}`);
  console.log(`  the router says:       routed spend = ${formatUsdc(spent)}`);
  console.log(
    `  ${formatUsdc(spent)} > ${formatUsdc(BOUND)} — and both numbers are on-chain state.`,
  );
  console.log("");
  note("Gross routed flow is NOT loss. This does not say $600 was stolen, or that");
  note("anyone is owed $600. It says the operator promised a ceiling and their own");
  note("agent's metered conduct went past it. The contract sizes no payout from it.");
  console.log("");

  // First, the branch that costs the challenger money.
  //
  // A false claim is not merely refused: it is *settled* at filing, in the same
  // transaction, and the bond is gone. That is only possible because the
  // predicate is arithmetic — the contract does not need anybody's opinion to
  // know that a fully funded reserve is fully funded. Filing this one here, on
  // a certificate whose reserve is demonstrably intact, is the cheapest honest
  // way to show the no-fraud branch working on live state rather than in a test.
  //
  // Order matters. It has to go before the true claim, because once a window is
  // open every later claim joins it instead of settling on its own.
  const challengerBefore = await bound.usdcBalance(challenger.publicKey());
  const falseClaim = await bound.challengeCertificate(challenger, {
    certId,
    proofType: "InsufficientReserve",
    victim: counterparty,
    bond: CHALLENGE_BOND,
  });
  const challengerAfter = await bound.usdcBalance(challenger.publicKey());
  ok(
    `false claim #${falseClaim} filed — reserve is ${formatUsdc(await bound.reserveBalance(certId))} against a claim of ${formatUsdc(RESERVE)}`,
  );
  ok(
    `rejected and settled in the same transaction: challenger ${formatUsdc(challengerBefore)} → ${formatUsdc(challengerAfter)}`,
  );
  note(`the ${formatUsdc(CHALLENGE_BOND)} bond is forfeit. Nobody adjudicated it and nobody`);
  note("had to: the contract read the vault and did the subtraction itself.");
  console.log("");

  // Now the branch that is true.
  const challengeId = await bound.challengeCertificate(challenger, {
    certId,
    proofType: "BoundExceeded",
    victim: counterparty,
    bond: CHALLENGE_BOND,
  });
  ok(
    `challenge #${challengeId} filed with a ${formatUsdc(CHALLENGE_BOND)} bond of the challenger's own money`,
  );

  const closesAt = await bound.windowClosesAt(certId);
  const windowSeconds = await bound.claimWindowSeconds();
  ok(`a ${Number(windowSeconds) / 3600}-hour claim window is now open on certificate #${certId}`);
  note(`it may be closed at ${new Date(Number(closesAt) * 1000).toISOString()}`);

  // ---------------------------------------------------------------------------
  act("What the other two proofs would read, right now");
  // ---------------------------------------------------------------------------
  // Printed rather than filed. One challenge per run is enough to show the
  // mechanism, and filing three against one certificate would only demonstrate
  // the aggregation rule, which the contract tests already cover exhaustively.
  console.log(
    `  InsufficientReserve — claimed ${formatUsdc(RESERVE)} vs live vault ${formatUsdc(await bound.reserveBalance(certId))}`,
  );
  console.log(`                        equal, which is why the claim above was rejected`);
  console.log(
    `  ExpiredCertificate  — expires ${new Date(Number(expiresAt) * 1000).toISOString()}`,
  );
  console.log(`                        now     ${new Date().toISOString()}`);
  console.log(`                        unexpired, so this proof would be rejected the same way`);
  console.log(`  FakeSignature       — no on-chain trace to read. The one proof that still`);
  console.log(`                        needs an arbiter, and the only one.`);
  console.log("");
  note("The first three are read from the same kind of state: a number the contract");
  note("already holds, checked against another number the contract already holds.");
  note("That is what shrank the arbiter down to signature forgery alone.");

  writeFileSync(
    STATE_PATH,
    JSON.stringify(
      {
        certId: Number(certId),
        challengeId: Number(challengeId),
        agent: agent.publicKey(),
        boundStroops: BOUND.toString(),
        spentStroops: spent.toString(),
        windowClosesAt: Number(closesAt),
        premiumStroops: quote.toString(),
        filedAt: new Date().toISOString(),
      },
      null,
      2,
    ) + "\n",
  );

  console.log("\n" + "═".repeat(62));
  console.log(`  Certificate #${certId} · challenge #${challengeId} · window open`);
  console.log("═".repeat(62));
  console.log(`
  Nothing has settled, and that is correct. The claim window exists so a
  claimant who files second is not foreclosed by one who filed first: every
  admitted claim against this certificate is priced together when the window
  lapses.

  Run \x1b[1mpnpm run demo:settle\x1b[0m after
  ${new Date(Number(closesAt) * 1000).toISOString()}
  to close the window, settle every claim at once, and see the auditor's yield.

  Run state written to .demo-run.json
`);
}

main().catch((err) => {
  console.error(`\n\x1b[31m✗ ${err?.message ?? err}\x1b[0m`);
  process.exit(1);
});
