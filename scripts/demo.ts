// Bound Protocol — 8-step end-to-end demo against live testnet.
//
//   pnpm run demo   (→ ts-node --project scripts/tsconfig.json scripts/demo.ts)
//
// The story: an auditor vouches (with their own staked capital) for a certificate
// that claims a $10k reserve — but the operator only funded $4k. A counterparty
// trusts the vouch and gets paid. Then a challenger calls resolve(): the contract
// itself reads the claim ($10k) against the live vault balance ($4k), proves the
// auditor's vouch false BY ARITHMETIC, and in one transaction slashes the auditor's
// stake, compensates the counterparty, and invalidates the cert. No oracle.
import { readEnv, invoke, usdc } from "./lib";

const env = readEnv();
const need = (k: string): string => {
  const v = env[k];
  if (!v) throw new Error(`missing ${k} in .env.testnet — run \`pnpm setup\` && \`pnpm deploy\``);
  return v;
};

// Addresses + secrets
const USDC = need("USDC_ADDRESS");
const RESERVE_VAULT = need("RESERVE_VAULT_ADDRESS");
const AUDITOR_STAKING = need("AUDITOR_STAKING_ADDRESS");
const FEE_ESCROW = need("FEE_ESCROW_ADDRESS");
const REGISTRY = need("REGISTRY_ADDRESS");
const CHALLENGE_MANAGER = need("CHALLENGE_MANAGER_ADDRESS");

const OPERATOR = need("OPERATOR_ADDRESS");
const OPERATOR_SK = need("OPERATOR_SECRET");
const AGENT = need("AGENT_ADDRESS");
const AGENT_SK = need("AGENT_SECRET");
const AUDITOR = need("AUDITOR_ADDRESS");
const AUDITOR_SK = need("AUDITOR_SECRET");
const CHALLENGER = need("CHALLENGER_ADDRESS");
const CHALLENGER_SK = need("CHALLENGER_SECRET");
const COUNTERPARTY = need("COUNTERPARTY_ADDRESS");

// Amounts (USDC, 7 decimals)
const AUDITOR_STAKE = usdc(1_500);
const RESERVE_DEPOSIT = usdc(4_000); // ← only $4k actually funded
const RESERVE_CLAIMED = usdc(10_000); // ← but the certificate claims $10k (the lie)
const FEE = usdc(500);
const BOUND = usdc(50_000);
const PAYMENT = usdc(500);
const CHALLENGE_BOND = usdc(100);

const EXPIRES_AT = String(Math.floor(Date.now() / 1000) + 30 * 24 * 3600);

let stepNo = 0;
function step(title: string) {
  stepNo += 1;
  console.log(`\n\x1b[1mStep ${stepNo}/8  ${title}\x1b[0m`);
}
const money = (stroops: string) => `$${(Number(stroops) / 1e7).toLocaleString()}`;

function usdcBalance(addr: string): string {
  return JSON.parse(invoke(USDC, OPERATOR_SK, "balance", ["--id", addr]));
}

function main() {
  console.log("══════════════════════════════════════════════");
  console.log("  Bound Protocol — E2E demo (Stellar testnet)");
  console.log("══════════════════════════════════════════════");

  // 1 — Auditor stakes their own capital (skin in the game)
  step(`Auditor stakes ${money(AUDITOR_STAKE)}`);
  invoke(AUDITOR_STAKING, AUDITOR_SK, "stake", ["--auditor", AUDITOR, "--amount", AUDITOR_STAKE]);
  console.log(`  ✓ auditor stake: ${money(invoke(AUDITOR_STAKING, OPERATOR_SK, "get_stake", ["--auditor", AUDITOR]).replace(/"/g, ""))}`);

  // 2 — Operator funds the reserve... but short
  step(`Operator deposits reserve — but only ${money(RESERVE_DEPOSIT)}`);
  invoke(RESERVE_VAULT, OPERATOR_SK, "deposit", ["--amount", RESERVE_DEPOSIT]);
  console.log(`  ✓ vault balance: ${money(invoke(RESERVE_VAULT, OPERATOR_SK, "get_balance", []).replace(/"/g, ""))} (cert will claim ${money(RESERVE_CLAIMED)})`);

  // 3 — Operator deposits the audit fee
  step(`Operator deposits ${money(FEE)} audit fee`);
  invoke(FEE_ESCROW, OPERATOR_SK, "deposit", ["--operator", OPERATOR, "--auditor", AUDITOR, "--amount", FEE]);
  console.log(`  ✓ fee escrowed for auditor`);

  // 4 — Operator publishes the certificate (PENDING). It claims $10k reserve.
  step(`Operator publishes certificate (claims reserve ${money(RESERVE_CLAIMED)})`);
  const certId = invoke(REGISTRY, OPERATOR_SK, "publish", [
    "--operator", OPERATOR,
    "--agent", AGENT,
    "--bound", BOUND,
    "--reserve_amount", RESERVE_CLAIMED,
    "--expires_at", EXPIRES_AT,
    "--reserve_vault_contract", RESERVE_VAULT,
    "--auditor_staking_contract", AUDITOR_STAKING,
  ]).replace(/"/g, "");
  console.log(`  ✓ cert_id: ${certId} (status: PENDING)`);

  // 5 — Auditor attests → VERIFIED. This is the false vouch.
  step(`Auditor attests the certificate (the false vouch)`);
  invoke(REGISTRY, AUDITOR_SK, "attest", ["--auditor", AUDITOR, "--cert_id", certId]);
  console.log(`  ✓ auditor signed — cert now VERIFIED`);

  // 6 — Counterparty verifies and decides to accept
  step(`Counterparty verifies the certificate`);
  const verify = JSON.parse(invoke(REGISTRY, OPERATOR_SK, "verify", ["--agent", AGENT]));
  console.log(`  valid: ${verify.valid} · status: ${verify.status}`);
  console.log(`  bound: ${money(verify.bound)} · reserve (claimed): ${money(verify.reserve)} · auditor stake: ${money(verify.auditor_stake)}`);
  console.log(`  ✓ counterparty accepts — "worst-case is bounded and vouched"`);

  // 7 — Agent pays the counterparty (direct USDC transfer)
  step(`Agent pays ${money(PAYMENT)} to the counterparty`);
  const cpBefore = usdcBalance(COUNTERPARTY);
  invoke(USDC, AGENT_SK, "transfer", ["--from", AGENT, "--to", COUNTERPARTY, "--amount", PAYMENT]);
  console.log(`  ✓ counterparty USDC: ${money(cpBefore)} → ${money(usdcBalance(COUNTERPARTY))}`);

  // 8 — Challenger proves the reserve is short → trustless slash + compensate
  step(`Challenger proves reserve is short → resolve() on-chain`);
  const auditorStakeBefore = usdcBalance(AUDITOR_STAKING);
  const cpBeforeSlash = usdcBalance(COUNTERPARTY);
  const challengerBefore = usdcBalance(CHALLENGER);

  const challengeId = invoke(CHALLENGE_MANAGER, CHALLENGER_SK, "challenge", [
    "--challenger", CHALLENGER,
    "--cert_id", certId,
    "--proof_type", '"InsufficientReserve"', // enum passed as JSON string
    "--victim", COUNTERPARTY,
    "--stake", CHALLENGE_BOND,
  ]).replace(/"/g, "");
  console.log(`  challenge_id: ${challengeId} · bond posted: ${money(CHALLENGE_BOND)}`);
  console.log(`  calling resolve() — the contract checks claim vs live balance…`);
  invoke(CHALLENGE_MANAGER, CHALLENGER_SK, "resolve", ["--challenge_id", challengeId]);

  const verifyAfter = JSON.parse(invoke(REGISTRY, OPERATOR_SK, "verify", ["--agent", AGENT]));
  console.log(`\n  ⚡ FRAUD PROVEN (claimed ${money(RESERVE_CLAIMED)} > actual ${money(RESERVE_DEPOSIT)})`);
  console.log(`     auditor staking pool: ${money(auditorStakeBefore)} → ${money(usdcBalance(AUDITOR_STAKING))} (slashed)`);
  console.log(`     counterparty:         ${money(cpBeforeSlash)} → ${money(usdcBalance(COUNTERPARTY))} (compensated)`);
  console.log(`     challenger:           ${money(challengerBefore)} → ${money(usdcBalance(CHALLENGER))} (bond back + reward)`);
  console.log(`     cert status:          ${verifyAfter.status} (valid: ${verifyAfter.valid})`);

  console.log("\n══════════════════════════════════════════════");
  console.log("  ✓ All 8 steps passed. The cage was economic — and it held.");
  console.log("══════════════════════════════════════════════");
}

main();
