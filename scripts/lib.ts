// Shared helpers for the deploy/setup scripts.
import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { resolve } from "node:path";

export const ENV_PATH = resolve(__dirname, "..", ".env.testnet");
export const NETWORK = "testnet";

// USDC on Stellar uses 7 decimals — $1 = 1_0000000 stroops.
export const USDC_DECIMALS = 7;
export const usdc = (dollars: number): string =>
  (BigInt(Math.round(dollars * 100)) * BigInt(10) ** BigInt(USDC_DECIMALS - 2)).toString();

// --- .env.testnet read/write -------------------------------------------------

export function readEnv(): Record<string, string> {
  if (!existsSync(ENV_PATH)) throw new Error(`.env.testnet not found at ${ENV_PATH}`);
  const out: Record<string, string> = {};
  for (const line of readFileSync(ENV_PATH, "utf8").split("\n")) {
    const m = line.match(/^([A-Z0-9_]+)=(.*)$/);
    if (m) out[m[1]] = m[2];
  }
  return out;
}

export function writeEnvValues(updates: Record<string, string>): void {
  let content = readFileSync(ENV_PATH, "utf8");
  for (const [key, value] of Object.entries(updates)) {
    const re = new RegExp(`^${key}=.*$`, "m");
    content = re.test(content)
      ? content.replace(re, `${key}=${value}`)
      : `${content.trimEnd()}\n${key}=${value}\n`;
  }
  writeFileSync(ENV_PATH, content);
}

// --- stellar CLI wrappers ----------------------------------------------------

// Synchronous sleep (the CLI calls are blocking, so we stay sync throughout).
function sleepMs(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

const TRANSIENT = /connection reset|connect error|timed out|timeout|temporarily|503|502|429|reset by peer|broken pipe/i;

// `capture` is kept for call-site clarity; stdout is always returned.
function stellar(args: string[], _capture: boolean): string {
  const maxAttempts = 4;
  for (let attempt = 1; ; attempt++) {
    const r = spawnSync("stellar", args, { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });

    if (r.error) {
      throw new Error(`stellar ${args.slice(0, 3).join(" ")} … could not run: ${r.error.message}`);
    }

    const stdout = r.stdout ?? "";
    const stderr = r.stderr ?? "";

    if (r.status === 0) {
      if (stderr) process.stderr.write(stderr); // forward progress logs
      return stdout;
    }

    const combined = `${stdout}\n${stderr}`;
    if (attempt < maxAttempts && TRANSIENT.test(combined)) {
      console.warn(`  ↻ retrying (${attempt}/${maxAttempts - 1}) after transient network error…`);
      sleepMs(1500 * attempt);
      continue;
    }

    if (stderr) process.stderr.write(stderr); // surface the real CLI error
    throw new Error(`stellar ${args.slice(0, 3).join(" ")} … failed (exit ${r.status})`);
  }
}

/** Deploy a wasm file. Returns the new contract id (C...). */
export function deploy(wasmPath: string, sourceSecret: string): string {
  const out = stellar(
    ["contract", "deploy", "--wasm", wasmPath, "--source-account", sourceSecret, "--network", NETWORK],
    true,
  );
  const m = out.match(/C[A-Z2-7]{55}/);
  if (!m) throw new Error(`could not parse contract id from deploy output:\n${out}`);
  return m[0];
}

/** Invoke a contract function. Returns the CLI's stdout (JSON for reads). */
export function invoke(contractId: string, sourceSecret: string, fn: string, args: string[]): string {
  return stellar(
    [
      "contract", "invoke",
      "--id", contractId,
      "--source-account", sourceSecret,
      "--network", NETWORK,
      "--", fn, ...args,
    ],
    false,
  ).trim();
}

/** Deterministic SAC id for a classic asset (no transaction). */
export function assetSacId(asset: string): string {
  const out = stellar(["contract", "id", "asset", "--asset", asset, "--network", NETWORK], true);
  const m = out.match(/C[A-Z2-7]{55}/);
  if (!m) throw new Error(`could not resolve SAC id for ${asset}:\n${out}`);
  return m[0];
}

/** Deploy (idempotently) the Stellar Asset Contract wrapping `asset`, return its id. */
export function deployAssetSac(asset: string, sourceSecret: string): string {
  const id = assetSacId(asset); // deterministic — same for any given asset/network
  // Probe: a read only succeeds if the contract is already deployed.
  try {
    stellar(
      ["contract", "invoke", "--id", id, "--source-account", sourceSecret, "--network", NETWORK, "--", "decimals"],
      true,
    );
    return id; // already deployed
  } catch {
    // not deployed yet — deploy it
  }
  stellar(
    ["contract", "asset", "deploy", "--asset", asset, "--source-account", sourceSecret, "--network", NETWORK],
    true,
  );
  return id;
}

/** Open a trustline so the source account can hold `line` (e.g. "USDC:G..."). */
export function changeTrust(line: string, sourceSecret: string): void {
  stellar(["tx", "new", "change-trust", "--line", line, "--source-account", sourceSecret, "--network", NETWORK], false);
}

/** Mint `amount` of the SAC asset to `to`. Must be signed by the issuer. */
export function mint(sac: string, issuerSecret: string, to: string, amount: string): void {
  invoke(sac, issuerSecret, "mint", ["--to", to, "--amount", amount]);
}

/** Initialize a contract, tolerating the "already initialized" case on re-runs. */
export function initialize(contractId: string, sourceSecret: string, args: string[]): void {
  try {
    invoke(contractId, sourceSecret, "initialize", args);
  } catch (err: any) {
    if (/already_initialized/.test(err.message)) {
      console.warn(`  ⚠ ${contractId} already initialized — skipping`);
      return;
    }
    throw err;
  }
}
