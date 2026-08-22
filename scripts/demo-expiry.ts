// Bound Protocol — the ExpiredCertificate proof, against live Stellar testnet.
//
//   pnpm run demo:expiry    — run it three times; it knows which act is next
//
// Three runs rather than one because this proof is made of two waits that the
// contracts impose and a demo may not shorten:
//
//   act 1  arm      publish a short-term certificate and enroll its agent
//   act 2  file     ~24h later: route a late payment, then prove the expiry
//   act 3  settle   ~72h after that: close the window and settle
//
// `pnpm run demo` proves BoundExceeded — a certificate whose agent outspent its
// own bound. This one proves the other half of the same claim: that a
// certificate nobody renewed is still being spent against. Both are read from
// on-chain state by arithmetic, and neither needs an arbiter.
//
// Why the waits are real. `verify_expired_certificate` demands three things
// (challenge-manager/src/lib.rs), and the middle one is a deliberate 24-hour
// grace window: a hostile counterparty who invoices an agent one second after
// expiry must not be able to kill an honest certificate over it. So the late
// payment has to land after `expires_at + 24h`, and the claim it proves then
// opens the same 72-hour window every other claim gets.
//
// This run uses its own fresh agent and its own certificate. It must: the
// predicate's third condition is that the certificate is still the agent's
// *current* one, so pointing a second certificate at the agent already carrying
// the BoundExceeded claim would quietly unprove that claim instead.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { Keypair } from "@stellar/stellar-sdk";
import { bound, contracts, formatUsdc, usdc } from "@bound/sdk";
import { readEnv, changeTrust, invoke } from "./lib";

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

// Amounts (USDC, 7 decimals). Deliberately smaller than the BoundExceeded
// demo's: nothing here needs to exceed a bound, so the certificate only has to
// be large enough that the de-minimis floor is a real number.
const BOUND = usdc(500);
const RESERVE = usdc(500); // funded in full: this operator is not lying about money
// The auditor's slice bonded to THIS certificate. It cannot be smaller: the
// AuditorStaking contract's `get_min_stake` is $500 and `allocate` panics with
// `allocation_below_minimum` under it. An auditor's bond is meant to hurt.
const ALLOCATION = usdc(500);
const FLOAT_CAP = usdc(10);
const LATE_PAYMENT = usdc(5); // 1% of the bound, vs a de-minimis floor of 0.1%
const CHALLENGE_BOND = usdc(500); // the ChallengeManager's minimum, and not small
const AGENT_FUNDING = usdc(20);

// Half an hour. The term only has to be short enough that expiry is behind us
// before the grace window starts running — every minute of term is a minute
// added to the 24h wait — but not so short that a failure midway through act 1
// leaves the certificate expiring before the retry can attest it.
const TERM_SECONDS = 1_800;

// Must match GRACE_WINDOW_SECONDS in contracts/challenge-manager/src/lib.rs.
// Read there rather than assumed here would be better; it is a constant, not a
// config value, so there is nothing on-chain to read it from.
const GRACE_SECONDS = 24 * 60 * 60;

const STATE_PATH = resolve(__dirname, "..", ".expiry-run.json");
const FRIENDBOT = "https://friendbot.stellar.org";

const ok = (line: string) => console.log(`  \x1b[32m✓\x1b[0m ${line}`);
const note = (line: string) => console.log(`    ${line}`);
const money = (stroops: bigint) => `${formatUsdc(stroops)} (${stroops.toLocaleString()} stroops)`;
const act = (n: number, title: string) => console.log(`\n\x1b[1mAct ${n}/3  ${title}\x1b[0m`);

/**
 * Written after every transaction, not at the end of the act.
 *
 * The first attempt at this script generated the agent keypair, published a
 * certificate against it, and then failed on `attest` — stranding a funded
 * certificate whose agent's secret existed only in a dead process's memory.
 * A run that spends real money persists what it spent it on before it spends
 * the next thing.
 */
interface RunState {
  agentSecret: string;
  agentPublic: string;
  certId?: number;
  expiresAt?: number;
  provableAt?: number;
  boundStroops?: string;
  premiumStroops?: string;
  reserveFunded?: boolean;
  attested?: boolean;
  enrolled?: boolean;
  armedAt?: string;
  challengeId?: number;
  windowClosesAt?: number;
  filedAt?: string;
  settledAt?: string;
}

const readState = (): RunState | null =>
  existsSync(STATE_PATH) ? (JSON.parse(readFileSync(STATE_PATH, "utf8")) as RunState) : null;
const writeState = (s: RunState) => writeFileSync(STATE_PATH, JSON.stringify(s, null, 2) + "\n");

const now = () => Math.floor(Date.now() / 1000);

function waitLine(untilTs: number, what: string): void {
  const left = untilTs - now();
  const hours = Math.floor(left / 3600);
  const minutes = Math.round((left % 3600) / 60);
  console.log(`\n  Not yet. ${hours}h ${minutes}m left before ${what}.`);
  console.log(`  Come back after ${new Date(untilTs * 1000).toISOString()} and run this again.\n`);
}

/**
 * The router's post-expiry counter, read through the stellar CLI.
 *
 * BoundClient does not surface `post_expiry_spent`, and @bound/sdk does not
 * re-export the v2 router binding either — `bindings.ts` lists the six v1
 * clients and was never extended when the router and the premium vault landed.
 * Worth fixing in the SDK; not worth blocking this proof on.
 */
interface PostExpiry {
  total: string;
  count: number;
  first_at: number;
  max_payment: string;
  max_payment_at: number;
}

function postExpiry(certId: number): PostExpiry {
  if (!contracts.paymentRouter) throw new Error("this deployment predates the PaymentRouter");
  const out = invoke(contracts.paymentRouter, operator.secret(), "post_expiry_spent", [
    "--cert_id",
    String(certId),
  ]);
  return JSON.parse(out) as PostExpiry;
}

async function friendbotFund(publicKey: string): Promise<void> {
  const res = await fetch(`${FRIENDBOT}?addr=${encodeURIComponent(publicKey)}`);
  if (!res.ok) {
    throw new Error(`friendbot failed for ${publicKey}: ${res.status} ${await res.text()}`);
  }
}

// --- act 1 -------------------------------------------------------------------

async function arm(prior: RunState | null): Promise<void> {
  act(1, "A short-term certificate is published, and its agent enrolled");

  let state: RunState;
  if (prior) {
    state = prior;
    ok(`resuming the certificate armed earlier — agent ${state.agentPublic}`);
  } else {
    const agent = Keypair.random();
    console.log(`  agent: ${agent.publicKey()}`);
    state = { agentSecret: agent.secret(), agentPublic: agent.publicKey() };
    writeState(state);
    await friendbotFund(agent.publicKey());
    changeTrust(`USDC:${operator.publicKey()}`, agent.secret());
    await bound.mintUsdc(operator, agent.publicKey(), AGENT_FUNDING);
    ok(`funded with XLM and ${formatUsdc(AGENT_FUNDING)} test USDC`);
  }
  const agent = Keypair.fromSecret(state.agentSecret);

  if (!state.certId) {
    const expiresAt = BigInt(now() + TERM_SECONDS);
    const certId = await bound.publishCertificate(operator, agent, {
      bound: BOUND,
      reserveAmount: RESERVE,
      expiresAt,
    });
    state = {
      ...state,
      certId: Number(certId),
      expiresAt: Number(expiresAt),
      provableAt: Number(expiresAt) + GRACE_SECONDS,
      boundStroops: BOUND.toString(),
    };
    writeState(state);
    ok(
      `certificate #${certId} published — bound ${formatUsdc(BOUND)}, term ${TERM_SECONDS / 60}min`,
    );
    note(`expires at ${new Date(Number(expiresAt) * 1000).toISOString()}`);
  }
  const certId = BigInt(state.certId!);

  if (!state.reserveFunded) {
    await bound.depositReserve(operator, certId, RESERVE);
    state = { ...state, reserveFunded: true };
    writeState(state);
    ok(`reserve funded in full: ${formatUsdc(await bound.reserveBalance(certId))}`);
    note("in full on purpose. A short-funded reserve would make InsufficientReserve");
    note("provable too, and a certificate that two proofs kill is no evidence about");
    note("which one killed it.");
  }

  if (!state.attested) {
    await bound.attestCertificate(auditor, certId, ALLOCATION);
    state = { ...state, attested: true };
    writeState(state);
    const verified = await bound.verifyCertificate(agent.publicKey());
    ok(`auditor attested ${formatUsdc(ALLOCATION)} — certificate is ${verified.status.tag}`);
    note("attestation is what the predicate's first condition reads. An unattested");
    note("certificate is not something anyone was promised, so letting it expire");
    note("proves nothing and pays no bounty.");
  }

  if (!state.premiumStroops) {
    const quote = await bound.quotePremiumForCert(certId);
    await bound.payPremium(operator, certId);
    state = { ...state, premiumStroops: quote.toString() };
    writeState(state);
    ok(`coverage bought for ${money(quote)} — protocol fee captured in the same transaction`);
  }

  if (!state.enrolled) {
    await bound.enrollAgent(operator, agent, certId, FLOAT_CAP);
    state = { ...state, enrolled: true, armedAt: new Date().toISOString() };
    writeState(state);
    ok(`agent enrolled against certificate #${certId}, float cap ${formatUsdc(FLOAT_CAP)}`);
    note("enrollment snapshots `expires_at`, and that snapshot is what decides");
    note("whether a later payment counts as post-expiry. No payment is made now:");
    note("this certificate's whole story is the one that arrives late.");
  }

  console.log("\n" + "═".repeat(62));
  console.log(`  Certificate #${state.certId} armed. The clock is the protocol's, not the demo's.`);
  console.log("═".repeat(62));
  console.log(`
  It expires at ${new Date(state.expiresAt! * 1000).toISOString()} and becomes
  challengeable ${GRACE_SECONDS / 3600}h after that, at ${new Date(state.provableAt! * 1000).toISOString()}.

  Run \x1b[1mpnpm run demo:expiry\x1b[0m again after that to route the late payment
  and file the proof.
`);
}

// --- act 2 -------------------------------------------------------------------

async function file(state: RunState): Promise<void> {
  act(2, "A payment lands after the grace window, and the expiry is proven");

  if (now() <= state.provableAt!) {
    waitLine(state.provableAt!, "a late payment can prove anything");
    console.log("  That wait is the grace window doing its job: a $1 invoice one second");
    console.log("  after expiry must not be able to kill an honest certificate.\n");
    return;
  }

  const certId = BigInt(state.certId!);
  const agent = Keypair.fromSecret(state.agentSecret);
  const floor = BigInt(state.boundStroops!) / 1000n; // 0.1% of the bound

  const receipt = await bound.executePayment(agent, counterparty, LATE_PAYMENT);
  if (!receipt.routed) {
    throw new Error(
      "the payment bypassed the router, so no post-expiry counter moved and " +
        "ExpiredCertificate stays unprovable",
    );
  }
  ok(`${formatUsdc(LATE_PAYMENT)} routed — ${new Date().toISOString()}`);

  const pe = postExpiry(state.certId!);
  ok(
    `router's post-expiry counter: ${pe.count} payment(s), largest ${money(BigInt(pe.max_payment))}`,
  );
  console.log("");
  console.log(
    `  the certificate says:  expired at   ${new Date(state.expiresAt! * 1000).toISOString()}`,
  );
  console.log(
    `  the router says:       paid at      ${new Date(pe.max_payment_at * 1000).toISOString()}`,
  );
  console.log(
    `  the grace window says: not before   ${new Date(state.provableAt! * 1000).toISOString()}`,
  );
  console.log(
    `  the de-minimis floor:  ${formatUsdc(floor)} — this payment was ${formatUsdc(BigInt(pe.max_payment))}`,
  );
  console.log("");
  note("Three numbers the contract already holds, and a comparison. Nobody is");
  note("asked whether the certificate lapsed; the ledger says when it did.");

  const challengeId = await bound.challengeCertificate(challenger, {
    certId,
    proofType: "ExpiredCertificate",
    victim: counterparty,
    bond: CHALLENGE_BOND,
  });
  ok(
    `challenge #${challengeId} filed with a ${formatUsdc(CHALLENGE_BOND)} bond of the challenger's own money`,
  );

  const closesAt = Number(await bound.windowClosesAt(certId));
  ok(`a ${Number(await bound.claimWindowSeconds()) / 3600}-hour claim window is now open`);

  writeState({
    ...state,
    challengeId: Number(challengeId),
    windowClosesAt: closesAt,
    filedAt: new Date().toISOString(),
  });

  console.log("\n" + "═".repeat(62));
  console.log(`  Certificate #${state.certId} · challenge #${challengeId} · window open`);
  console.log("═".repeat(62));
  console.log(`
  Run \x1b[1mpnpm run demo:expiry\x1b[0m again after
  ${new Date(closesAt * 1000).toISOString()} to settle it.
`);
}

// --- act 3 -------------------------------------------------------------------

async function settle(state: RunState): Promise<void> {
  act(3, "The window lapses and the certificate is killed");

  const certId = BigInt(state.certId!);

  if (await bound.claimWindowSettled(certId)) {
    ok("this certificate's window is already closed and settled");
  } else {
    if (now() < state.windowClosesAt!) {
      waitLine(state.windowClosesAt!, "the claim window lapses");
      return;
    }
    const before = await bound.usdcBalance(challenger.publicKey());
    await bound.closeClaimWindow(challenger, certId);
    const after = await bound.usdcBalance(challenger.publicKey());
    ok("window closed — the claim settled with no arbiter in the path");
    ok(
      `challenger ${formatUsdc(before)} → ${formatUsdc(after)}: bond returned plus the hygiene bounty`,
    );
  }

  const verdict = await bound.verifyCertificate(state.agentPublic);
  ok(`certificate is now ${verdict.status.tag}, valid: ${verdict.valid}`);
  note("Hygiene mode, exactly as for BoundExceeded: the certificate is killed and");
  note("the challenger is paid a flat bounty for ending it. Letting a certificate");
  note("lapse while still spending against it is a broken covenant, not a theft,");
  note("and the auditor's stake is not slashed for it.");

  console.log(`  reserve untouched:     ${formatUsdc(await bound.reserveBalance(certId))}`);
  console.log(
    `  auditor stake:         ${formatUsdc(await bound.auditorStake(auditor.publicKey()))}`,
  );

  writeState({ ...state, settledAt: new Date().toISOString() });

  console.log("\n" + "═".repeat(62));
  console.log("  ExpiredCertificate: proven and settled on live testnet.");
  console.log("═".repeat(62) + "\n");
}

async function main() {
  console.log("═".repeat(62));
  console.log("  Bound Protocol — the ExpiredCertificate proof (Stellar testnet)");
  console.log("═".repeat(62));

  const state = readState();
  if (!state) return arm(null);
  if (!state.enrolled) return arm(state);
  if (state.settledAt) {
    console.log(`\n  Certificate #${state.certId} is already settled (${state.settledAt}).`);
    console.log(`  Delete ${STATE_PATH} to arm a new one.\n`);
    return;
  }
  if (!state.challengeId) return file(state);
  return settle(state);
}

main().catch((err) => {
  console.error(`\n\x1b[31m✗ ${err?.message ?? err}\x1b[0m`);
  process.exit(1);
});
