import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { TESTNET } from "../src/testnet.js";
import { VERSION } from "../src/version.js";
import { DEPLOYMENT_PATH, loadDeployment } from "../tooling/deployment.js";
import { summarize } from "../tooling/summary.js";

describe("published package data", () => {
  it("VERSION matches package.json", () => {
    const pkg = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
    expect(VERSION).toBe(pkg.version);
  });
  it.skipIf(!existsSync(DEPLOYMENT_PATH))(
    "bundled data/testnet.json matches near/deployments (run sync-deployment)",
    () => {
      expect(TESTNET).toEqual(summarize(loadDeployment()));
    },
  );
  it("bundles no key material", () => {
    const raw = readFileSync(new URL("../data/testnet.json", import.meta.url), "utf8");
    expect(raw).not.toMatch(/ed25519:|private_key|secret/i);
  });
});
