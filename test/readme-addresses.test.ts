// The README's address table must be the deployment record, not a copy of it.
//
// This exists because the copy had already drifted. The table listed the v1
// addresses long after the v2 deploy replaced them, and omitted PaymentRouter
// and PremiumVault entirely — so every address a reader could paste into an
// explorer pointed at a contract that is not the one this repo describes. A
// stale address in a README is worse than no address: it is a claim that fails
// on the first click, by someone checking whether the project is real.
//
// Prose can drift and nobody notices. A test does not.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

import { getDeployment } from "../packages/sdk/src/deployments";

const readme = readFileSync(fileURLToPath(new URL("../README.md", import.meta.url)), "utf8");

const deployment = getDeployment();

/** Every `C...` contract id the README states, wherever it states it. */
function addressesIn(text: string): string[] {
  return [...new Set(text.match(/C[A-Z2-7]{55}/g) ?? [])];
}

describe("README addresses", () => {
  const deployed = Object.values(deployment.contracts).filter(
    (id): id is string => typeof id === "string",
  );

  it("states every deployed contract", () => {
    for (const [name, id] of Object.entries(deployment.contracts)) {
      if (!id) continue;
      expect(readme, `${name} (${id}) is missing from README.md`).toContain(id);
    }
  });

  it("states no contract id that is not in the deployment record", () => {
    // The failure this catches by name: an address left behind by a previous
    // deploy, still sitting in the table looking authoritative.
    for (const id of addressesIn(readme)) {
      expect(deployed, `README.md names ${id}, which no deployment record has`).toContain(id);
    }
  });

  it("names the deploy commit it was generated from", () => {
    expect(readme).toContain(deployment.deployCommit.slice(0, 12));
  });

  it("states the read source account", () => {
    expect(readme).toContain(deployment.readSource);
  });

  it("leaks no secret key", () => {
    // Same guard the deployment record carries. The README is public and is the
    // file most likely to be edited by hand.
    expect(readme).not.toMatch(/\bS[A-Z2-7]{55}\b/);
  });
});
