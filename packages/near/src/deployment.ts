// The NEAR testnet deployment record: near/deployments/testnet.json.
// Written by scripts/deploy.ts, read by everything else. Holds no secrets.

import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
import type { KeyPairString } from "near-api-js";

const here = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(here, "../../..");
export const NEAR_DIR = join(REPO_ROOT, "near");
export const DEPLOYMENT_PATH = join(NEAR_DIR, "deployments", "testnet.json");
export const EVIDENCE_DIR = join(NEAR_DIR, "evidence");
export const DEMO_KEYS_DIR = join(NEAR_DIR, ".demo-keys");

export interface TxRef {
  step: string;
  hash: string;
  url: string;
}

export interface NearDeployment {
  network: "testnet";
  deployedAt: string;
  deployer: string;
  contracts: {
    bound: { accountId: string; wasmSha256: string; codeHash: string; wasmBytes: number };
    token: {
      accountId: string;
      wasmSha256: string;
      codeHash: string;
      wasmBytes: number;
      note: string;
    };
  };
  accounts: { treasury: string; arbiter: string; owner: string };
  config: Record<string, unknown>;
  rpc: string[];
  txs: TxRef[];
  demoRuns: Array<{
    runId: string;
    evidence: string;
    agent: string;
    agent2: string;
    certId: number;
    cert2Id: number;
  }>;
}

export function loadDeployment(path = DEPLOYMENT_PATH): NearDeployment {
  if (!existsSync(path)) {
    throw new Error(`no deployment record at ${path}; run pnpm near:deploy first`);
  }
  return JSON.parse(readFileSync(path, "utf8")) as NearDeployment;
}

export function saveDeployment(d: NearDeployment, path = DEPLOYMENT_PATH): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(d, null, 2) + "\n");
}

/** near-cli legacy keychain file (~/.near-credentials/testnet/<id>.json). */
export function credentialPath(accountId: string): string {
  return join(homedir(), ".near-credentials", "testnet", `${accountId}.json`);
}

export function readCredential(accountId: string): KeyPairString {
  const p = credentialPath(accountId);
  if (!existsSync(p)) throw new Error(`no NEAR credential for ${accountId} at ${p}`);
  const j = JSON.parse(readFileSync(p, "utf8")) as { private_key?: string; secret_key?: string };
  const k = j.private_key ?? j.secret_key;
  if (!k) throw new Error(`credential file for ${accountId} has no private_key`);
  return k as KeyPairString;
}

export function writeCredential(accountId: string, publicKey: string, privateKey: string): void {
  const p = credentialPath(accountId);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(
    p,
    JSON.stringify({ account_id: accountId, public_key: publicKey, private_key: privateKey }) +
      "\n",
    { mode: 0o600 },
  );
}
