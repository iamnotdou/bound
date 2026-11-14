// pnpm near:demo -- the Bound lifecycle on NEAR testnet, end to end.
//
// Creates fresh demo actors with the public testnet faucet (no keys needed),
// runs acts 1-8 against the deployed contract, ASSERTS every expected balance
// delta, prints a NearBlocks link per transaction, and writes
// near/evidence/near-demo-<runid>.txt + .json. Exits non-zero on any mismatch.
//
// Flags:
//   --fresh        also create a faucet deployer and deploy a private copy of
//                  both contracts first, so the whole demo needs no credential
//                  at all (the faucet deployer is the arbiter). Default: use
//                  near/deployments/testnet.json, which needs the arbiter key
//                  in ~/.near-credentials.
//   --no-evidence  do not write evidence files or update the deployment record.
//   --no-faucet    skip the faucet; create actors through the testnet registrar
//                  funded by --funder <id> (default boundproto-nh26.testnet,
//                  THE AUTHOR'S account: a reviewer passes its own), key read
//                  from ~/.near-credentials/testnet. Also the automatic
//                  fallback when the faucet rate-limits; without that key the
//                  run stops with a message saying so.

import { mkdirSync, writeFileSync, appendFileSync } from "node:fs";
import { join } from "node:path";
import {
  Account,
  JsonRpcProvider,
  KeyPair,
  actions,
  nearToYocto,
  type KeyPairString,
  type Provider,
} from "near-api-js";
import {
  MILLINEAR,
  ONE_YOCTO,
  TESTNET_RPCS,
  TGAS,
  accountFromKey,
  chainNow,
  accountCodeOf,
  decodeOutcome,
  errorText,
  testnetProvider,
  tryCall,
  view,
  withRetry,
  type TxOutcome,
} from "../src/client.js";
import {
  DEMO_KEYS_DIR,
  EVIDENCE_DIR,
  loadDeployment,
  readCredential,
  saveDeployment,
  type NearDeployment,
} from "../src/deployment.js";
import { deployAll } from "../src/deploy-lib.js";
import {
  NO_CODE_HASH,
  describeCert,
  explorerAccount,
  fmtUsdc,
  globalContractOf,
  usdc,
  type CertViewJson,
} from "../src/format.js";
import { Gate } from "../src/gate.js";

const FAUCET = "https://helper.nearprotocol.com/account";
const fresh = process.argv.includes("--fresh");
const writeEvidence = !process.argv.includes("--no-evidence");
const runId = Date.now().toString(36);
const startedAt = new Date().toISOString();

mkdirSync(EVIDENCE_DIR, { recursive: true });
mkdirSync(DEMO_KEYS_DIR, { recursive: true });
const txtPath = join(EVIDENCE_DIR, `near-demo-${runId}.txt`);
const jsonPath = join(EVIDENCE_DIR, `near-demo-${runId}.json`);
const keysPath = join(DEMO_KEYS_DIR, `${runId}.json`);
// Gate state is evidence for a recorded run; for --no-evidence it stays in the
// gitignored keys dir.
const gateDir = writeEvidence ? EVIDENCE_DIR : DEMO_KEYS_DIR;
const gateFiles = {
  ledger: join(gateDir, `gate-${runId}-ledger.json`),
  invoices: join(gateDir, `gate-${runId}-invoices.json`),
};

function out(s = "") {
  console.log(s);
  if (writeEvidence) appendFileSync(txtPath, s + "\n");
}

interface StepRec {
  act: number;
  label: string;
  signer: string;
  receiver: string;
  method: string;
  hash: string | null;
  url: string | null;
  ok: boolean;
  expectOk: boolean;
  expectError?: string;
  error: string | null;
  pass: boolean;
  note?: string;
}
interface AssertRec {
  act: number;
  label: string;
  expected: string;
  actual: string;
  pass: boolean;
}
const steps: StepRec[] = [];
const asserts: AssertRec[] = [];
const extra: Record<string, unknown> = {};
let act = 0;

function header(n: number, title: string) {
  act = n;
  out("");
  out(`== Act ${n}: ${title}`);
}

function record(
  label: string,
  o: TxOutcome,
  expectOk: boolean,
  expectError?: string,
  note?: string,
): TxOutcome {
  const pass = o.ok === expectOk && (!expectError || (o.error ?? "").includes(expectError));
  steps.push({
    act,
    label,
    signer: o.signer,
    receiver: o.receiver,
    method: o.method,
    hash: o.hash,
    url: o.url,
    ok: o.ok,
    expectOk,
    expectError,
    error: o.error,
    pass,
    note,
  });
  const mark = pass ? "OK  " : "FAIL";
  const res = o.ok ? "succeeded" : `rejected: ${o.error}`;
  out(`[${mark}] ${label}: ${res}`);
  if (o.url) out(`       tx ${o.url}`);
  else out(`       no transaction exists: the network refused it before execution`);
  if (note) out(`       ${note}`);
  return o;
}

function check(
  label: string,
  actual: bigint | string | number | boolean,
  expected: bigint | string | number | boolean,
) {
  const a = typeof actual === "bigint" ? fmtUsdc(actual) : String(actual);
  const e = typeof expected === "bigint" ? fmtUsdc(expected) : String(expected);
  const pass = a === e;
  asserts.push({ act, label, expected: e, actual: a, pass });
  out(`[${pass ? "OK  " : "FAIL"}] assert ${label}: ${a}${pass ? "" : ` (expected ${e})`}`);
}

function argValue(name: string): string | undefined {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : undefined;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function faucetAccount(provider: Provider, id: string, key: KeyPair): Promise<string> {
  const r = await fetch(FAUCET, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ newAccountId: id, newAccountPublicKey: key.getPublicKey().toString() }),
  });
  const body = await r.text();
  if (!r.ok) throw new Error(`faucet ${r.status}: ${body.slice(0, 200)}`);
  for (let i = 0; i < 20; i++) {
    try {
      await provider.viewAccount({ accountId: id, blockQuery: { finality: "final" } } as never);
      const hash =
        (JSON.parse(body) as { transaction?: { hash?: string } }).transaction?.hash ?? "";
      return hash;
    } catch {
      await sleep(1500);
    }
  }
  throw new Error(`faucet account ${id} never appeared`);
}

async function main() {
  const provider = testnetProvider();
  const keys: Record<string, { accountId: string; secretKey: string }> = {};
  const saveKeys = () => writeFileSync(keysPath, JSON.stringify(keys, null, 2), { mode: 0o600 });

  out(`Bound on NEAR testnet: demo run ${runId} (started ${startedAt})`);
  out(`Every [OK] line was checked by this script; any FAIL makes it exit 1.`);

  // ------------------------------------------------------------ actors
  header(1, "actors, mock USDC, registration");
  // Actors come from the public faucet. If it refuses (it rate-limits per IP),
  // fall back to top-level accounts created through the `testnet` registrar
  // and funded by a funder account from the local near-cli keychain
  // (default: the deployer), so the run still proves the same thing.
  const funderId = argValue("funder") ?? "boundproto-nh26.testnet";
  let funder: Account | null = null;
  let faucetDown = process.argv.includes("--no-faucet");
  const make = async (role: string) => {
    const id = `bound-${role}-${runId}.testnet`;
    const kp = KeyPair.fromRandom("ed25519");
    let how = "faucet";
    let hash = "";
    if (!faucetDown) {
      try {
        hash = await faucetAccount(provider, id, kp);
      } catch (e) {
        faucetDown = true;
        out(`  faucet refused (${errorText(e).slice(0, 80)}); falling back to ${funderId}`);
      }
    }
    if (faucetDown) {
      if (!funder) {
        let key: KeyPairString;
        try {
          key = readCredential(funderId);
        } catch {
          throw new Error(
            `the public faucet refused (it rate-limits per IP for hours) and there is no key for ` +
              `the funder ${funderId} in ~/.near-credentials/testnet. The default funder is the ` +
              `author's account. Pass --funder <your-testnet-account> (about 10 testnet NEAR for ` +
              `--fresh, 4 without), or retry later. The zero-secret checks (near:status, ` +
              `near:verify, near:evidence) need no account at all.`,
          );
        }
        funder = accountFromKey(provider, funderId, key);
        out(`  creating actors through the testnet registrar, funded by ${funderId}`);
      }
      const near = role === "deployer" ? "6" : "0.5";
      const r = (await funder.createAccount({
        newAccountId: id,
        publicKey: kp.getPublicKey(),
        nearToTransfer: BigInt(nearToYocto(near as `${number}`)),
      })) as unknown as { transaction: { hash: string } };
      hash = r.transaction.hash;
      how = `registrar, ${near} NEAR from ${funderId}`;
      for (let i = 0; i < 20; i++) {
        try {
          await provider.viewAccount({ accountId: id, blockQuery: { finality: "final" } } as never);
          break;
        } catch {
          await sleep(1500);
        }
      }
    }
    keys[role] = { accountId: id, secretKey: kp.toString() };
    saveKeys();
    out(`  ${role.padEnd(10)} ${id} (${how}) ${explorerAccount(id)}${hash ? ` tx ${hash}` : ""}`);
    return accountFromKey(provider, id, kp.toString() as KeyPairString);
  };

  let dep: NearDeployment;
  let arbiter: Account;
  if (fresh) {
    const deployer = await make("deployer");
    out(`  --fresh: deploying a private copy of both contracts from ${deployer.accountId}`);
    dep = await deployAll(provider, deployer, {
      log: (s) => out(s),
      boundNear: "3.4",
      tokenNear: "1.8",
    });
    if (writeEvidence) saveDeployment(dep, join(EVIDENCE_DIR, `fresh-deployment-${runId}.json`));
    arbiter = accountFromKey(provider, dep.accounts.arbiter, readCredential(dep.accounts.arbiter));
  } else {
    dep = loadDeployment();
    arbiter = accountFromKey(provider, dep.accounts.arbiter, readCredential(dep.accounts.arbiter));
  }
  const B = dep.contracts.bound.accountId;
  const T = dep.contracts.token.accountId;
  const treasury = dep.accounts.treasury;
  out(`  contract   ${B}`);
  out(`  token      ${T} (mock USDC, open mint, play money)`);
  out(`  treasury   ${treasury}   arbiter ${dep.accounts.arbiter}`);

  const operator = await make("operator");
  const agentFull = await make("agent");
  const auditor = await make("auditor");
  const challenger = await make("challenger");
  const service = await make("service");
  const agent2 = await make("agent2");
  const attacker = `bound-attacker-${runId}.testnet`; // never created; it only receives
  extra.actors = {
    operator: operator.accountId,
    agent: agentFull.accountId,
    auditor: auditor.accountId,
    challenger: challenger.accountId,
    service: service.accountId,
    agent2: agent2.accountId,
    attacker,
    arbiter: arbiter.accountId,
    treasury,
  };

  const bal = async (id: string) =>
    BigInt(await view<string>(provider, B, "balance_of", { account_id: id }));
  const ft = async (id: string) =>
    BigInt(await view<string>(provider, T, "ft_balance_of", { account_id: id }));
  const stakeOf = async (id: string) => {
    const s = await view<{ free: string; allocated: string }>(provider, B, "get_stake", {
      auditor: id,
    });
    return { free: BigInt(s.free), allocated: BigInt(s.allocated) };
  };
  const certOf = async (agent: string) =>
    view<CertViewJson | null>(provider, B, "verify", { agent });
  const y = { deposit: ONE_YOCTO };

  const mint = async (who: Account, amount: string) =>
    record(
      `mint ${amount} mUSDC to ${who.accountId}`,
      await tryCall(operator, T, "mint", {
        account_id: who.accountId,
        amount: usdc(amount).toString(),
      }),
      true,
    );
  const register = async (who: Account) =>
    record(
      `${who.accountId} storage_deposit on bound (0.01 NEAR)`,
      await tryCall(who, B, "storage_deposit", {}, { deposit: 10n * MILLINEAR }),
      true,
    );
  const depositTo = async (who: Account, amount: string, msg: string, label: string) =>
    record(
      label,
      await tryCall(
        who,
        T,
        "ft_transfer_call",
        { receiver_id: B, amount: usdc(amount).toString(), msg },
        { deposit: ONE_YOCTO, gas: 100n * TGAS },
      ),
      true,
    );

  const treasury0 = await bal(treasury);
  await mint(operator, "1330"); // 1300 deposited now, 30 sent later with the reserve action
  await mint(auditor, "600");
  await mint(challenger, "50");
  await mint(service, "10");
  for (const a of [operator, auditor, challenger, service]) await register(a);

  // ------------------------------------------------------------ cert
  header(2, "stake, publish, accept, fund, attest, verify");
  await depositTo(auditor, "600", '{"stake":{}}', "auditor stakes 600 via ft_transfer_call");
  await depositTo(operator, "1300", "", "operator deposits 1300 via ft_transfer_call");
  await depositTo(challenger, "50", "", "challenger deposits 50");
  await depositTo(service, "10", "", "service deposits 10 (for its claim bond)");
  check("auditor free stake", (await stakeOf(auditor.accountId)).free, usdc("600"));

  const now0 = await chainNow(provider);
  const pub = record(
    "operator publishes cert: bound 150, reserve 100, float cap 20, allowlist [service], premium 3, expires +1h",
    await tryCall(
      operator,
      B,
      "publish",
      {
        agent: agentFull.accountId,
        bound: usdc("150").toString(),
        reserve: usdc("100").toString(),
        expires_at: now0 + 3600,
        auditor: auditor.accountId,
        float_cap: usdc("20").toString(),
        allowlist: [service.accountId],
        premium: usdc("3").toString(),
      },
      { deposit: 50n * MILLINEAR },
    ),
    true,
  );
  const certId = Number(pub.value);
  out(`       cert_id ${certId}`);
  record(
    "agent accepts (its full-access key, the last time it is used)",
    await tryCall(agentFull, B, "accept", { cert_id: certId }, y),
    true,
  );
  record(
    "operator funds reserve 100",
    await tryCall(
      operator,
      B,
      "fund_reserve",
      { cert_id: certId, amount: usdc("100").toString() },
      y,
    ),
    true,
  );
  record(
    "operator funds float 20",
    await tryCall(operator, B, "fund_float", { cert_id: certId, amount: usdc("20").toString() }, y),
    true,
  );
  record(
    "auditor attests, locking 200 of its stake behind this cert",
    await tryCall(auditor, B, "attest", { cert_id: certId, allocation: usdc("200").toString() }, y),
    true,
  );
  const v1 = await certOf(agentFull.accountId);
  out(`  verify(${agentFull.accountId}) -> ${v1 ? describeCert(v1) : "null"}`);
  check("verify status", v1?.status ?? "null", "Verified");
  check("premium: auditor balance +2.7", await bal(auditor.accountId), usdc("2.7"));
  check("premium: treasury +0.3", (await bal(treasury)) - treasury0, usdc("0.3"));
  check("operator balance 1300 - 3 - 100 - 20", await bal(operator.accountId), usdc("1177"));

  // ------------------------------------------------------------ confinement
  header(3, "confine the agent to one function-call key; the gate");
  const fcKey = KeyPair.fromRandom("ed25519");
  const addKey = await agentFull.signAndSendTransaction({
    receiverId: agentFull.accountId,
    actions: [actions.addFunctionCallAccessKey(fcKey.getPublicKey(), B, ["pay"], 250n * MILLINEAR)],
    waitUntil: "FINAL",
    throwOnFailure: false,
  });
  record(
    `agent adds a function-call key: receiver ${B}, methods ["pay"], allowance 0.25 NEAR (gas only)`,
    decodeOutcome(addKey as never, agentFull.accountId, agentFull.accountId, "AddKey"),
    true,
  );
  const fullKeyPub = KeyPair.fromString(keys.agent.secretKey as KeyPairString).getPublicKey();
  const delKey = await agentFull.signAndSendTransaction({
    receiverId: agentFull.accountId,
    actions: [actions.deleteKey(fullKeyPub)],
    waitUntil: "FINAL",
    throwOnFailure: false,
  });
  record(
    "agent DELETES its full-access key",
    decodeOutcome(delKey as never, agentFull.accountId, agentFull.accountId, "DeleteKey"),
    true,
  );
  keys.agentFc = { accountId: agentFull.accountId, secretKey: fcKey.toString() };
  saveKeys();
  const agent = accountFromKey(provider, agentFull.accountId, fcKey.toString() as KeyPairString);
  // Same key, single RPC (no failover): a key-level refusal then surfaces the
  // node's own error class instead of a failover summary.
  const agentDirect = accountFromKey(
    new JsonRpcProvider({ url: TESTNET_RPCS[0] }),
    agentFull.accountId,
    fcKey.toString() as KeyPairString,
  );
  const keyList = (await withRetry(() =>
    provider.viewAccessKeyList({ accountId: agent.accountId, finality: "final" } as never),
  )) as unknown as { keys: Array<{ public_key: string; access_key: { permission: unknown } }> };
  extra.agentKeyList = keyList.keys;
  out(`  view_access_key_list(${agent.accountId}):`);
  out(`    ${JSON.stringify(keyList.keys)}`);
  const only = keyList.keys.length === 1 ? keyList.keys[0] : null;
  const perm = only?.access_key.permission as
    { FunctionCall?: { receiver_id: string; method_names: string[] } } | undefined;
  check("agent has exactly one key", keyList.keys.length, 1);
  check(
    "that key is FunctionCall scoped to the bound contract",
    perm?.FunctionCall?.receiver_id ?? "none",
    B,
  );
  check(
    "its only method is pay",
    JSON.stringify(perm?.FunctionCall?.method_names ?? []),
    '["pay"]',
  );
  // Keys are not the whole story: contract code on the agent account would
  // run as the agent with no key at all. Confined = pay-only keys AND no code.
  // A NEAR global contract also shows code_hash all ones, so the
  // global_contract_* fields must be empty too.
  const agentAcct = await accountCodeOf(provider, agent.accountId);
  const agentCode = agentAcct.code_hash;
  extra.agentCodeHash = agentCode;
  out(`  view_account(${agent.accountId}).code_hash = ${agentCode}`);
  check("no contract code on the agent account (code_hash all ones)", agentCode, NO_CODE_HASH);
  check(
    "no global contract on the agent account (global_contract_hash and global_contract_account_id null)",
    globalContractOf(agentAcct) ?? "none",
    "none",
  );

  const gate = new Gate(provider, B, service.accountId, usdc("5"), gateFiles);
  const g0 = await gate.handle({ agent: agent2.accountId });
  out(`  gate <- ${agent2.accountId} (no certificate): ${g0.status} ${g0.reason}`);
  check("gate refuses an uncertified agent", g0.status, 403);

  const pay1 = record(
    "agent pays service 5 through bound pay (the function-call key, no deposit)",
    await tryCall(agent, B, "pay", {
      cert_id: certId,
      to: service.accountId,
      amount: usdc("5").toString(),
      memo: `gate:req-${runId}-1`,
    }),
    true,
  );
  const g1 = await gate.handle({ agent: agent.accountId, payTx: pay1.hash ?? "" });
  out(`  gate <- ${agent.accountId} with payment ${pay1.hash}: ${g1.status} ${g1.reason}`);
  check("gate serves the prepaid request", g1.status, 200);
  const g2 = await gate.handle({ agent: agent.accountId, payTx: pay1.hash ?? "" });
  out(`  gate <- same payment hash again: ${g2.status} ${g2.reason}`);
  check("gate refuses a replayed payment hash", g2.status, 409);
  const g3 = await gate.handle({
    agent: agent.accountId,
    credit: usdc("120"),
    description: "batch inference job, post-paid",
  });
  out(
    `  gate <- ${agent.accountId} asks for 120 on credit: ${g3.status} ${g3.reason} (${g3.invoiceId})`,
  );
  check("gate serves the certified agent on credit", g3.status, 200);
  const invoiceId = g3.invoiceId ?? "inv-?";
  extra.gate = { refused: g0, prepaid: g1, replay: g2, credit: g3, invoices: gate.invoices() };

  // ------------------------------------------------------------ attacks
  // Scripted: these are the exact calls a prompt-injected agent would make
  // with the key it holds. No language model runs in this demo.
  header(4, "scripted attack: the calls a hijacked agent would make with its key");
  record(
    "agent pays 50 to service (float is 15)",
    await tryCall(agent, B, "pay", {
      cert_id: certId,
      to: service.accountId,
      amount: usdc("50").toString(),
      memo: "injected",
    }),
    false,
    "FloatExceeded",
  );
  record(
    `agent pays 10 to ${attacker}`,
    await tryCall(agent, B, "pay", {
      cert_id: certId,
      to: attacker,
      amount: usdc("10").toString(),
      memo: "injected",
    }),
    false,
    "RecipientNotAllowed",
  );
  record(
    "same key calls ft_transfer on the token",
    await tryCall(
      agentDirect,
      T,
      "ft_transfer",
      { receiver_id: attacker, amount: "1" },
      { deposit: ONE_YOCTO, gas: 30n * TGAS },
    ),
    false,
    "DepositWithFunctionCall",
    "refused by NEAR itself: a function-call key cannot attach a deposit, and ft_transfer needs 1 yocto",
  );
  record(
    "same key calls mint on the token (no deposit needed)",
    await tryCall(
      agentDirect,
      T,
      "mint",
      { account_id: attacker, amount: "1" },
      { gas: 30n * TGAS },
    ),
    false,
    "ReceiverMismatch",
    "refused by NEAR itself: the key may only call the bound contract",
  );
  record(
    "same key calls bound withdraw (needs 1 yocto)",
    await tryCall(
      agentDirect,
      B,
      "withdraw",
      { amount: "1" },
      { deposit: ONE_YOCTO, gas: 50n * TGAS },
    ),
    false,
    "DepositWithFunctionCall",
    "refused by NEAR itself: no deposit from a function-call key (and withdraw is not in its method list)",
  );
  let nearTransfer: TxOutcome;
  try {
    const r = await agentDirect.signAndSendTransaction({
      receiverId: attacker,
      actions: [actions.transfer(10n ** 22n)],
      waitUntil: "FINAL",
      throwOnFailure: false,
    });
    nearTransfer = decodeOutcome(r as never, agent.accountId, attacker, "Transfer");
  } catch (e) {
    nearTransfer = {
      hash: null,
      url: null,
      ok: false,
      error: errorText(e),
      logs: [],
      value: undefined,
      signer: agent.accountId,
      receiver: attacker,
      method: "Transfer",
    };
  }
  record(
    "same key sends 0.01 NEAR to the attacker",
    nearTransfer,
    false,
    "RequiresFullAccess",
    "refused by NEAR itself: a function-call key cannot sign a transfer",
  );
  for (const s of steps.slice(-4)) {
    if (s.hash !== null) {
      s.pass = false;
      out(`[FAIL] expected a key-level refusal with no transaction for "${s.label}"`);
    }
  }
  record(
    "operator halts the cert",
    await tryCall(operator, B, "halt", { cert_id: certId }, y),
    true,
  );
  record(
    "agent pays 1 to service while halted",
    await tryCall(agent, B, "pay", {
      cert_id: certId,
      to: service.accountId,
      amount: usdc("1").toString(),
    }),
    false,
    "CertHalted",
  );
  out(`  the agent never pays invoice ${invoiceId} (120 USDC)`);

  // ------------------------------------------------------------ false claim
  header(5, "a false trustless claim is rejected in the same transaction");
  const t5 = await bal(treasury);
  const fc = record(
    "challenger files BoundExceeded (bond 10) while spent 5 <= bound 150",
    await tryCall(
      challenger,
      B,
      "challenge",
      { cert_id: certId, proof: "BoundExceeded", bond: usdc("10").toString() },
      { deposit: 10n * MILLINEAR },
    ),
    true,
  );
  const fch = await view<{ verdict: string }>(provider, B, "get_challenge", {
    challenge_id: Number(fc.value),
  });
  check("verdict", fch.verdict, "Rejected");
  check("bond forfeited to treasury (+10)", (await bal(treasury)) - t5, usdc("10"));
  check("challenger balance 50 - 10", await bal(challenger.accountId), usdc("40"));

  // ------------------------------------------------------------ harm
  header(6, "the service claims its unpaid invoice; the arbiter decides");
  const evidenceUri = `near/evidence/gate-${runId}-invoices.json#${invoiceId}`;
  const hc = record(
    `service files Harm 120 citing ${evidenceUri} (bond 10)`,
    await tryCall(
      service,
      B,
      "challenge",
      {
        cert_id: certId,
        proof: { Harm: { claimed: usdc("120").toString(), evidence_uri: evidenceUri } },
        bond: usdc("10").toString(),
      },
      { deposit: 10n * MILLINEAR },
    ),
    true,
  );
  const harmId = Number(hc.value);
  record(
    "anyone tries to close the window right away",
    await tryCall(challenger, B, "close_window", { cert_id: certId }, y),
    false,
    "WindowOpen",
  );
  record(
    "arbiter resolves: proven, harm 120",
    await tryCall(
      arbiter,
      B,
      "resolve_by_arbiter",
      { challenge_id: harmId, proven: true, harm: usdc("120").toString() },
      y,
    ),
    true,
  );
  check("verify status", (await certOf(agent.accountId))?.status ?? "null", "Challenged");
  record(
    "operator resumes (so the next refusal is the freeze, not the halt)",
    await tryCall(operator, B, "resume", { cert_id: certId }, y),
    true,
  );
  record(
    "agent pays 1 while the claim window is open",
    await tryCall(agent, B, "pay", {
      cert_id: certId,
      to: service.accountId,
      amount: usdc("1").toString(),
    }),
    false,
    "CertFrozen",
  );

  // ------------------------------------------------------------ settle
  header(7, "the window closes; one atomic settlement");
  const w = await view<{ closes_at: number }>(provider, B, "get_window", { cert_id: certId });
  out(`  window closes at ${new Date(w.closes_at * 1000).toISOString()} (chain time); waiting`);
  for (;;) {
    const n = await chainNow(provider);
    if (n > w.closes_at) break;
    await sleep(5000);
  }
  const t7 = await bal(treasury);
  const op7 = await bal(operator.accountId);
  let cw = await tryCall(challenger, B, "close_window", { cert_id: certId }, y);
  for (let i = 0; i < 5 && !cw.ok && (cw.error ?? "").includes("WindowOpen"); i++) {
    await sleep(5000);
    cw = await tryCall(challenger, B, "close_window", { cert_id: certId }, y);
  }
  record("challenger (anyone) calls close_window", cw, true);
  const v7 = await certOf(agent.accountId);
  const s = (v7?.settlement ?? {}) as Record<string, string>;
  out(`  settlement: ${JSON.stringify(s)}`);
  check("cert status", v7?.status ?? "null", "Invalid");
  check("payable = min(120, 100 + 200)", BigInt(s.payable ?? 0), usdc("120"));
  check("victim paid in full: 100 reserve + 20 slash", BigInt(s.victim_paid ?? 0), usdc("120"));
  check("challenger fee = min(12, reserve left 0)", BigInt(s.fee_paid ?? 0), usdc("0"));
  check(
    "auditor slashed only the 20 the reserve could not cover",
    BigInt(s.slash ?? 0),
    usdc("20"),
  );
  check("the whole slash went to the victim", BigInt(s.slash_to_victims ?? 0), usdc("20"));
  check(
    "unslashed allocation (released to the auditor at the deadline)",
    BigInt(s.auditor_returned ?? 0),
    usdc("180"),
  );
  check("treasury +0 at settlement (nothing left over)", (await bal(treasury)) - t7, usdc("0"));
  check(
    "treasury over the run: 0.3 premium + 10 forfeited bond",
    (await bal(treasury)) - treasury0,
    usdc("10.3"),
  );
  const st = await stakeOf(auditor.accountId);
  // The unslashed 180 stays locked behind cert 0 until its settlement deadline
  // (expires_at + post_expiry); release_allocation then frees it (580 free).
  check("auditor free stake unchanged until the deadline", st.free, usdc("400"));
  check("unslashed 180 still allocated to cert 0 until the deadline", st.allocated, usdc("180"));
  check("unspent float 15 back to the operator", (await bal(operator.accountId)) - op7, usdc("15"));
  check(
    "service bound balance: 5 prepaid + 120 compensation + 10 bond back",
    await bal(service.accountId),
    usdc("135"),
  );
  const ft0 = await ft(service.accountId);
  const wd = record(
    "service withdraws 135 to its token account",
    await tryCall(
      service,
      B,
      "withdraw",
      { amount: usdc("135").toString() },
      { deposit: ONE_YOCTO, gas: 60n * TGAS },
    ),
    true,
  );
  check("withdraw callback reports success", String(wd.value), "true");
  check(
    "service ft_balance_of on the token rose by 135",
    (await ft(service.accountId)) - ft0,
    usdc("135"),
  );
  check("service bound balance after withdraw", await bal(service.accountId), usdc("0"));
  out(`  The service lost 120 and recovered 120: 100 from the operator's reserve, 20 from the`);
  out(`  auditor's slash. Coverage = reserve + allocation (300 here) caps what victims recover.`);
  const g7 = await gate.handle({ agent: agent.accountId, credit: usdc("1") });
  out(`  gate <- ${agent.accountId} after settlement: ${g7.status} ${g7.reason}`);
  check("gate refuses the settled agent", g7.status, 403);

  // ------------------------------------------------------------ cert 2
  header(8, "a second cert left Verified for judging (agent2, expires 2026-11-30)");
  const exp2 = Math.floor(Date.UTC(2026, 10, 30, 0, 0, 0) / 1000);
  const pub2 = record(
    "operator publishes cert 2 for agent2: bound 50, reserve 30, float cap 10, premium 1",
    await tryCall(
      operator,
      B,
      "publish",
      {
        agent: agent2.accountId,
        bound: usdc("50").toString(),
        reserve: usdc("30").toString(),
        expires_at: exp2,
        auditor: auditor.accountId,
        float_cap: usdc("10").toString(),
        allowlist: [service.accountId],
        premium: usdc("1").toString(),
      },
      { deposit: 50n * MILLINEAR },
    ),
    true,
  );
  const cert2Id = Number(pub2.value);
  record("agent2 accepts", await tryCall(agent2, B, "accept", { cert_id: cert2Id }, y), true);
  record(
    "operator funds reserve 30 (ft_transfer_call with the reserve action)",
    await tryCall(
      operator,
      T,
      "ft_transfer_call",
      {
        receiver_id: B,
        amount: usdc("30").toString(),
        msg: JSON.stringify({ reserve: { cert_id: cert2Id } }),
      },
      { deposit: ONE_YOCTO, gas: 100n * TGAS },
    ),
    true,
  );
  record(
    "operator funds float 10",
    await tryCall(
      operator,
      B,
      "fund_float",
      { cert_id: cert2Id, amount: usdc("10").toString() },
      y,
    ),
    true,
  );
  record(
    "auditor attests cert 2 with 100",
    await tryCall(
      auditor,
      B,
      "attest",
      { cert_id: cert2Id, allocation: usdc("100").toString() },
      y,
    ),
    true,
  );
  const v8 = await certOf(agent2.accountId);
  out(`  verify(${agent2.accountId}) -> ${v8 ? describeCert(v8) : "null"}`);
  check("cert 2 status", v8?.status ?? "null", "Verified");
  const g8 = await gate.handle({ agent: agent2.accountId });
  out(`  gate <- ${agent2.accountId} now certified, no payment: ${g8.status} ${g8.reason}`);
  check("gate now asks agent2 to pay instead of refusing", g8.status, 402);

  // ------------------------------------------------------------ summary
  const failedSteps = steps.filter((x) => !x.pass);
  const failedAsserts = asserts.filter((x) => !x.pass);
  out("");
  out(
    `== Summary: ${steps.length} steps (${steps.filter((x) => x.hash).length} transactions), ${asserts.length} assertions`,
  );
  out(`   failed steps: ${failedSteps.length}, failed assertions: ${failedAsserts.length}`);
  out(
    `   re-check every hash without NearBlocks: pnpm near:evidence near/evidence/near-demo-${runId}.json`,
  );

  if (writeEvidence) {
    writeFileSync(
      jsonPath,
      JSON.stringify(
        {
          runId,
          startedAt,
          finishedAt: new Date().toISOString(),
          network: "testnet",
          contract: B,
          token: T,
          fresh,
          certId,
          cert2Id,
          harmChallengeId: harmId,
          ...extra,
          steps,
          asserts,
          pass: failedSteps.length === 0 && failedAsserts.length === 0,
        },
        null,
        2,
      ) + "\n",
    );
    if (!fresh && failedSteps.length === 0 && failedAsserts.length === 0) {
      const d = loadDeployment();
      d.demoRuns.push({
        runId,
        evidence: `near/evidence/near-demo-${runId}.json`,
        agent: agent.accountId,
        agent2: agent2.accountId,
        certId,
        cert2Id,
        txs: steps
          .filter((x) => x.hash)
          .map((x) => ({ act: x.act, step: x.label, hash: x.hash!, url: x.url!, ok: x.ok })),
        keyLevelRefusals: steps
          .filter((x) => !x.hash)
          .map((x) => ({ act: x.act, step: x.label, error: x.error ?? "" })),
      });
      saveDeployment(d);
    }
    out(`   evidence: near/evidence/near-demo-${runId}.txt and .json`);
  }
  if (failedSteps.length || failedAsserts.length) process.exit(1);
}

main().catch((e) => {
  out(`FATAL: ${errorText(e)}`);
  process.exit(1);
});
