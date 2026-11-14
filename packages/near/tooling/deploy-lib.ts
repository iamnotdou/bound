// Deploy mock USDC + the Bound contract as sub-accounts of `parent`.
// Each contract is created, funded, keyed, deployed and initialized in ONE
// transaction (deploy-with-init), so nobody can race to call `new`.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { Account, KeyPair, actions, baseEncode, nearToYocto, type Provider } from "near-api-js";
import { NEAR_DIR, type NearDeployment, type TxRef, writeCredential } from "./deployment.js";
import { TGAS, decodeOutcome, TESTNET_RPCS } from "../src/client.js";
import { explorerTx } from "../src/format.js";

export const WASM = {
  bound: join(NEAR_DIR, "target/near/bound/bound.wasm"),
  token: join(NEAR_DIR, "target/near/mock_usdc/mock_usdc.wasm"),
};

/** Demo timers: claim window 90 s, post-expiry 300 s, grace 60 s.
 *  Production defaults would be 72 h / 7 d / 24 h. Amounts in USDC base units. */
export const DEMO_CONFIG = {
  claim_window_secs: 90,
  post_expiry_secs: 300,
  grace_secs: 60,
  min_stake: "10000000",
  min_challenge_bond: "10000000",
  challenger_fee_bps: 1000,
  de_minimis_bps: 10,
  hygiene_bounty: "10000000",
  protocol_fee_bps: 1000,
};

export function wasmInfo(path: string) {
  const code = readFileSync(path);
  const digest = createHash("sha256").update(code).digest();
  return { code, sha256: digest.toString("hex"), codeHash: baseEncode(digest), bytes: code.length };
}

async function createSub(
  parent: Account,
  sub: string,
  near: string,
  extra: ReturnType<typeof actions.functionCall>[] = [],
  code?: Uint8Array,
  log: (s: string) => void = console.log,
): Promise<{ hash: string; key: KeyPair }> {
  const key = KeyPair.fromRandom("ed25519");
  const acts = [
    actions.createAccount(),
    actions.transfer(BigInt(nearToYocto(near as `${number}`))),
    actions.addFullAccessKey(key.getPublicKey()),
    ...(code ? [actions.deployContract(code)] : []),
    ...extra,
  ];
  const r = await parent.signAndSendTransaction({
    receiverId: sub,
    actions: acts,
    waitUntil: "FINAL",
    throwOnFailure: false,
  });
  const o = decodeOutcome(
    r as never,
    parent.accountId,
    sub,
    code ? "create+deploy+init" : "create",
  );
  if (!o.ok || !o.hash) throw new Error(`creating ${sub} failed: ${o.error}`);
  writeCredential(sub, key.getPublicKey().toString(), key.toString());
  log(
    `  created ${sub} (${near} NEAR)${code ? " + deployed + initialized" : ""}: ${explorerTx(o.hash)}`,
  );
  return { hash: o.hash, key };
}

export interface DeployOpts {
  /** Sub-account names (prefix before the parent id). */
  boundName?: string;
  tokenName?: string;
  boundNear?: string;
  tokenNear?: string;
  log?: (s: string) => void;
  /** Reuse the token, treasury and arbiter sub-accounts when they already
   *  exist (redeploying only the Bound contract under a new name). */
  reuseExisting?: boolean;
}

async function exists(provider: Provider, id: string): Promise<boolean> {
  try {
    await provider.viewAccount({ accountId: id, blockQuery: { finality: "final" } } as never);
    return true;
  } catch {
    return false;
  }
}

export async function deployAll(
  provider: Provider,
  parent: Account,
  opts: DeployOpts = {},
): Promise<NearDeployment> {
  const log = opts.log ?? console.log;
  const p = parent.accountId;
  const boundId = `${opts.boundName ?? "bound-v1"}.${p}`;
  const tokenId = `${opts.tokenName ?? "usdc"}.${p}`;
  const treasury = `treasury.${p}`;
  const arbiter = `arbiter.${p}`;
  const bound = wasmInfo(WASM.bound);
  const token = wasmInfo(WASM.token);
  const txs: TxRef[] = [];
  const push = (step: string, hash: string) => txs.push({ step, hash, url: explorerTx(hash) });

  log(`Deploying from ${p}`);
  const reuse = async (id: string) => {
    if (opts.reuseExisting && (await exists(provider, id))) {
      log(`  reusing existing ${id}`);
      return true;
    }
    return false;
  };
  if (!(await reuse(tokenId))) {
    const t = await createSub(
      parent,
      tokenId,
      opts.tokenNear ?? "1.8",
      [actions.functionCall("new", {}, 30n * TGAS, 0n)],
      token.code,
      log,
    );
    push("deploy mock-usdc (create + deploy + new)", t.hash);
  }
  if (!(await reuse(treasury))) {
    const tr = await createSub(parent, treasury, "0.05", [], undefined, log);
    push("create treasury account", tr.hash);
  }
  if (!(await reuse(arbiter))) {
    const ar = await createSub(parent, arbiter, "0.3", [], undefined, log);
    push("create arbiter account", ar.hash);
  }
  const b = await createSub(
    parent,
    boundId,
    opts.boundNear ?? "3.4",
    [
      actions.functionCall(
        "new",
        { owner: p, token: tokenId, treasury, arbiter, config: DEMO_CONFIG },
        50n * TGAS,
        0n,
      ),
    ],
    bound.code,
    log,
  );
  push("deploy bound (create + deploy + new)", b.hash);

  // The bound contract needs a token account to receive ft_transfer_call.
  const reg = await parent.signAndSendTransaction({
    receiverId: tokenId,
    actions: [
      actions.functionCall("storage_deposit", { account_id: boundId }, 30n * TGAS, 5n * 10n ** 21n),
    ],
    waitUntil: "FINAL",
    throwOnFailure: false,
  });
  const ro = decodeOutcome(reg as never, p, tokenId, "storage_deposit");
  if (!ro.ok || !ro.hash) throw new Error(`registering bound on the token failed: ${ro.error}`);
  push("register bound on mock-usdc (storage_deposit)", ro.hash);
  log(`  registered ${boundId} on ${tokenId}: ${explorerTx(ro.hash)}`);

  // Prove the on-chain code is the wasm we built.
  for (const [id, w] of [
    [boundId, bound],
    [tokenId, token],
  ] as const) {
    const acc = (await provider.viewAccount({
      accountId: id,
      blockQuery: { finality: "final" },
    } as never)) as {
      code_hash: string;
    };
    if (acc.code_hash !== w.codeHash) {
      throw new Error(`${id} code_hash ${acc.code_hash} != local wasm ${w.codeHash}`);
    }
    log(`  ${id} code_hash ${acc.code_hash} matches local wasm sha256 ${w.sha256}`);
  }

  return {
    network: "testnet",
    deployedAt: new Date().toISOString(),
    deployer: p,
    contracts: {
      bound: {
        accountId: boundId,
        wasmSha256: bound.sha256,
        codeHash: bound.codeHash,
        wasmBytes: bound.bytes,
      },
      token: {
        accountId: tokenId,
        wasmSha256: token.sha256,
        codeHash: token.codeHash,
        wasmBytes: token.bytes,
        note: "Mock USDC, 6 decimals, OPEN mint: testnet play money, not Circle USDC.",
      },
    },
    accounts: { treasury, arbiter, owner: p },
    config: DEMO_CONFIG,
    rpc: TESTNET_RPCS,
    txs,
    demoRuns: [],
  };
}
