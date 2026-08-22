// Bound Protocol — regenerate docs/EVIDENCE.md from the ledger itself.
//
//   pnpm run evidence
//
// Every claim this project makes about working on-chain is a transaction hash
// or it is a press release. This script does not take the demo's word for what
// happened: it reads the accounts the demo used straight off Horizon, decodes
// which contract function each transaction actually called, and writes the
// table. Re-run it after any act of any demo and the document catches up.
//
// It is deliberately not fed by the demo scripts' own logs. A log says what a
// script believed it did; Horizon says what the network accepted.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";
import * as prettier from "prettier";
import { scValToNative, xdr } from "@stellar/stellar-sdk";
import { formatUsdc } from "@bound/sdk";
import { readEnv } from "./lib";

const ROOT = resolve(__dirname, "..");
const DEPLOYMENT = JSON.parse(readFileSync(resolve(ROOT, "deployments/testnet.json"), "utf8"));
const OUT_PATH = resolve(ROOT, "docs/EVIDENCE.md");
const HORIZON = DEPLOYMENT.horizonUrl as string;
const EXPLORER = "https://stellar.expert/explorer/testnet";

const env = readEnv();

/** contract address → the name a reader knows it by */
const CONTRACT_NAMES: Record<string, string> = Object.fromEntries(
  Object.entries(DEPLOYMENT.contracts as Record<string, string>).map(([name, addr]) => [
    addr,
    name
      .replace(/([A-Z])/g, " $1")
      .replace(/^./, (c) => c.toUpperCase())
      .trim(),
  ]),
);

interface Op {
  hash: string;
  at: string;
  contract: string;
  fn: string;
  args: unknown[];
  source: string;
}

async function horizon(path: string): Promise<any> {
  const res = await fetch(`${HORIZON}${path}`);
  if (!res.ok) throw new Error(`horizon ${path}: ${res.status} ${await res.text()}`);
  return res.json();
}

function decode(value: string): unknown {
  try {
    return scValToNative(xdr.ScVal.fromXDR(value, "base64"));
  } catch {
    return undefined;
  }
}

/** Every contract call an account made, newest last. */
async function opsFor(account: string): Promise<Op[]> {
  const out: Op[] = [];
  let cursor = "";
  for (;;) {
    const page = await horizon(
      `/accounts/${account}/operations?order=asc&limit=200${cursor ? `&cursor=${cursor}` : ""}`,
    );
    const records: any[] = page._embedded.records;
    if (records.length === 0) break;
    for (const r of records) {
      cursor = r.paging_token;
      if (r.type !== "invoke_host_function" || !r.parameters || r.parameters.length < 2) continue;
      if (r.transaction_successful === false) continue;
      const contract = decode(r.parameters[0].value);
      const fn = decode(r.parameters[1].value);
      if (typeof contract !== "string" || typeof fn !== "string") continue;
      out.push({
        hash: r.transaction_hash,
        at: r.created_at,
        contract,
        fn,
        args: r.parameters.slice(2).map((p: any) => decode(p.value)),
        source: account,
      });
    }
    if (records.length < 200) break;
  }
  return out;
}

const usd = (v: unknown): string => (typeof v === "bigint" ? formatUsdc(v) : String(v));

/**
 * What a transaction is evidence *of*. Anything not named here is real and
 * on-chain but is plumbing — funding a test account, opening a trustline —
 * and lands in the appendix rather than the headline tables.
 */
function describe(op: Op): string | null {
  const [, a1, a2] = op.args;
  switch (op.fn) {
    case "publish":
      return `Certificate published — bound ${usd(a2)}`;
    case "attest":
      return `Auditor attested, bonding ${usd(a2)} of their own slashable stake`;
    case "stake":
      return `Auditor staked ${usd(a1)}`;
    case "pay_premium":
      return "Premium deposited at publication — protocol fee captured in the same transaction";
    case "claim":
      return "Auditor claimed accrued yield on their staked capital";
    case "enroll":
      return `Agent enrolled in the PaymentRouter, float cap ${usd(a2)}`;
    case "challenge":
      return `Challenge filed — proof type ${proofOf(op)}, bond ${usd(op.args[4])}`;
    case "close_window":
      return "Claim window closed — every admitted claim settled at once, with no arbiter";
    case "resolve_by_arbiter":
      return "Arbiter resolution (FakeSignature path)";
    case "transfer":
      return op.contract === DEPLOYMENT.contracts.paymentRouter
        ? `Payment routed and metered — ${usd(a2)}`
        : null;
    case "deposit":
      return op.contract === DEPLOYMENT.contracts.reserveVault
        ? `Reserve funded — ${usd(a1)}`
        : null;
    default:
      return null;
  }
}

function proofOf(op: Op): string {
  const v = op.args[2];
  if (Array.isArray(v) && typeof v[0] === "string") return v[0];
  if (typeof v === "string") return v;
  return "unknown";
}

/**
 * Which argument is the certificate id, per function. Read off the contract
 * signatures rather than guessed: "the first small integer" also matches a
 * float cap, an allocation, or a payment amount, and silently invents
 * certificates that do not exist.
 *
 * `publish` is absent because it does not take a cert id — it returns one.
 */
const CERT_ARG: Record<string, number> = {
  attest: 1,
  deposit: 0, // ReserveVault.deposit(cert_id, amount)
  pay_premium: 0,
  claim: 0,
  enroll: 1,
  challenge: 1,
  close_window: 0,
  terminate: 0,
};

function certOf(op: Op): number | null {
  // The router's own `deposit(from, amount)` shares a name with the reserve
  // vault's `deposit(cert_id, amount)` and carries no certificate at all.
  if (op.fn === "deposit" && op.contract !== DEPLOYMENT.contracts.reserveVault) return null;
  const idx = CERT_ARG[op.fn];
  if (idx === undefined) return null;
  const v = op.args[idx];
  return typeof v === "bigint" ? Number(v) : null;
}

/**
 * `publish` returns the new certificate id rather than taking it, and Horizon's
 * operation record carries arguments but not return values — so the id is read
 * back out of the transaction's Soroban meta, which is where the network
 * recorded what the call actually returned.
 *
 * An earlier version of this guessed instead, by looking ahead to the next call
 * that named a certificate. It produced a table with two different transactions
 * both publishing #3. A document whose whole purpose is to be checkable does not
 * get to guess.
 */
async function publishedCertId(hash: string): Promise<number | null> {
  const res = await fetch(DEPLOYMENT.rpcUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "getTransaction", params: { hash } }),
  });
  const meta = (await res.json())?.result?.resultMetaXdr;
  if (!meta) return null; // outside the RPC's retention window
  try {
    const m: any = xdr.TransactionMeta.fromXDR(meta, "base64");
    // Protocol 23 writes v4; earlier ledgers wrote v3. Ask for both.
    const soroban = (() => {
      for (const v of ["v4", "v3"]) {
        try {
          const inner = m[v]();
          if (inner?.sorobanMeta()) return inner.sorobanMeta();
        } catch {
          /* wrong arm */
        }
      }
      return null;
    })();
    const rv = soroban?.returnValue?.();
    const v = rv ? scValToNative(rv) : null;
    return typeof v === "bigint" ? Number(v) : null;
  } catch {
    return null;
  }
}

/**
 * Certificates the demos recorded for themselves, keyed by the agent they were
 * published against. Horizon keeps operation arguments for ever but the RPC
 * only keeps transaction results for a few days, so this is what keeps older
 * publishes attributed once their meta has aged out.
 */
function certsByAgent(): Map<string, number> {
  const out = new Map<string, number>();
  for (const f of [".demo-run.json", ".expiry-run.json"]) {
    const p = resolve(ROOT, f);
    if (!existsSync(p)) continue;
    const s = JSON.parse(readFileSync(p, "utf8"));
    const agent = s.agent ?? s.agentPublic;
    if (agent && s.certId) out.set(agent, s.certId);
  }
  return out;
}

const publishCerts = new Map<string, number>();

const link = (hash: string) => `[\`${hash.slice(0, 12)}…\`](${EXPLORER}/tx/${hash})`;

function table(rows: Op[]): string {
  const lines = [
    "| When (UTC) | Cert | What it evidences | Contract · function | Transaction |",
    "| ---------- | ---- | ----------------- | ------------------- | ----------- |",
  ];
  for (const op of rows) {
    const what = describe(op);
    if (!what) continue;
    const cert = certOf(op) ?? publishCerts.get(op.hash) ?? null;
    lines.push(
      `| ${op.at.replace("T", " ").replace("Z", "")} | ${cert === null ? "—" : `#${cert}`} | ${what} | ${CONTRACT_NAMES[op.contract] ?? op.contract.slice(0, 8)} · \`${op.fn}\` | ${link(op.hash)} |`,
    );
  }
  return lines.join("\n");
}

async function main() {
  const accounts = new Set<string>([
    env.OPERATOR_ADDRESS,
    env.AUDITOR_ADDRESS,
    env.CHALLENGER_ADDRESS,
    env.COUNTERPARTY_ADDRESS,
  ]);
  for (const f of [".demo-run.json", ".expiry-run.json"]) {
    const p = resolve(ROOT, f);
    if (!existsSync(p)) continue;
    const s = JSON.parse(readFileSync(p, "utf8"));
    if (s.agent) accounts.add(s.agent);
    if (s.agentPublic) accounts.add(s.agentPublic);
  }

  const all: Op[] = [];
  for (const a of accounts) all.push(...(await opsFor(a)));

  // One transaction touches several of these accounts, so the same hash comes
  // back more than once. Keep the first sighting of each.
  const seen = new Set<string>();
  const ops = all
    .filter((o) => (seen.has(o.hash) ? false : (seen.add(o.hash), true)))
    .sort((x, y) => x.at.localeCompare(y.at));

  const boundExceeded = ops.filter((o) => o.fn === "challenge" && proofOf(o) === "BoundExceeded");
  const expired = ops.filter((o) => o.fn === "challenge" && proofOf(o) === "ExpiredCertificate");
  const insufficient = ops.filter(
    (o) => o.fn === "challenge" && proofOf(o) === "InsufficientReserve",
  );
  const settled = ops.filter((o) => o.fn === "close_window");
  const premium = ops.filter((o) => o.fn === "pay_premium" || o.fn === "claim");

  const knownAgents = certsByAgent();
  for (const op of ops.filter((o) => o.fn === "publish")) {
    const fromAgent = typeof op.args[1] === "string" ? knownAgents.get(op.args[1]) : undefined;
    const cert = (await publishedCertId(op.hash)) ?? fromAgent ?? null;
    if (cert !== null) publishCerts.set(op.hash, cert);
  }

  const certs = [...new Set(ops.map(certOf).filter((c): c is number => c !== null))].sort(
    (a, b) => a - b,
  );

  const doc = `# Evidence

Every transaction below is on Stellar testnet and clickable. This file is
generated by \`pnpm run evidence\`, which reads the accounts the demos used
straight off Horizon and decodes what each transaction actually called — so it
records what the network accepted, not what a script's log said it did.

Deployment: \`${DEPLOYMENT.deployCommit.slice(0, 12)}\`, ${DEPLOYMENT.deployedAt.slice(0, 10)}. Contracts:

${Object.entries(DEPLOYMENT.contracts as Record<string, string>)
  .map(([n, a]) => `- **${n}** — [\`${a}\`](${EXPLORER}/contract/${a})`)
  .join("\n")}

---

## R1 — the fraud proofs, settled without an arbiter

Three of the four proofs are read from on-chain state by arithmetic. The fourth,
\`FakeSignature\`, leaves no on-chain trace and is the one case that still needs an
arbiter — it is not evidenced here because there is nothing for a contract to read.

### BoundExceeded — routed spend past the certified bound

${boundExceeded.length ? table(boundExceeded) : "_No transaction yet._"}

### ExpiredCertificate — spend continuing after the certificate lapsed

${expired.length ? table(expired) : "_No transaction yet — the certificate is armed and inside its grace window. See “What is still on the clock”._"}

### InsufficientReserve — filed against an intact reserve, and rejected

A false claim, filed deliberately. It is settled inside its own filing
transaction and the bond is forfeit: a predicate the contract can evaluate
itself needs no claim window when it comes out false.

${insufficient.length ? table(insufficient) : "_No transaction yet._"}

### Settlement

${settled.length ? table(settled) : "_No claim window has been closed yet._"}

---

## R2 — the premium economy

\`pay_premium\` is both halves of the coverage economy in one transaction: the
operator's premium is deposited and the protocol's fee share is transferred to
the treasury inside the same call (\`premium-vault/src/lib.rs\`). The auditor's
yield is claimed separately, against accrual across the term.

${premium.length ? table(premium) : "_No transaction yet._"}

---

## Full ledger

Every contract call these demos made, in order.

${table(ops)}

Certificates covered: ${certs.map((c) => `#${c}`).join(", ")}.

---

## What is still on the clock

Settlement is not slow; it is windowed. A true claim opens a 72-hour window that
every other claimant against the same certificate may join, and the whole set is
priced together when it lapses. Nothing below is missing work.

${pending()}
`;

  // Formatted here rather than left for a human to tidy: `pnpm verify` runs
  // `prettier --check .`, so a generated file that is not already formatted
  // turns every regeneration into a red build.
  const config = await prettier.resolveConfig(OUT_PATH);
  writeFileSync(OUT_PATH, await prettier.format(doc, { ...config, parser: "markdown" }));
  console.log(`wrote ${OUT_PATH}`);
  console.log(`  ${ops.length} contract calls across ${accounts.size} accounts`);
  console.log(`  BoundExceeded: ${boundExceeded.length}  ExpiredCertificate: ${expired.length}`);
  console.log(`  InsufficientReserve: ${insufficient.length}  settlements: ${settled.length}`);
}

function pending(): string {
  const lines: string[] = [];
  for (const [file, label] of [
    [".demo-run.json", "BoundExceeded (`pnpm run demo:settle`)"],
    [".expiry-run.json", "ExpiredCertificate (`pnpm run demo:expiry`)"],
  ] as const) {
    const p = resolve(ROOT, file);
    if (!existsSync(p)) continue;
    const s = JSON.parse(readFileSync(p, "utf8"));
    if (s.settledAt) {
      lines.push(`- ${label} — settled ${s.settledAt.slice(0, 19).replace("T", " ")} UTC.`);
    } else if (s.windowClosesAt) {
      lines.push(
        `- ${label} — certificate #${s.certId}, claim filed, window closes ${new Date(s.windowClosesAt * 1000).toISOString().slice(0, 19).replace("T", " ")} UTC.`,
      );
    } else if (s.provableAt) {
      lines.push(
        `- ${label} — certificate #${s.certId} armed; the late payment cannot prove anything until the 24h grace window ends at ${new Date(s.provableAt * 1000).toISOString().slice(0, 19).replace("T", " ")} UTC, and the claim it then files opens its own 72h window.`,
      );
    }
  }
  return lines.length ? lines.join("\n") : "_Nothing pending._";
}

main().catch((err) => {
  console.error(err?.message ?? err);
  process.exit(1);
});
