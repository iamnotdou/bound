import { describe, it, expect } from "vitest";
import { getDeployment, listNetworks } from "./deployments";

describe("getDeployment()", () => {
  it("defaults to testnet", () => {
    expect(getDeployment().network).toBe("testnet");
  });

  it("returns the same object for an explicit testnet lookup", () => {
    expect(getDeployment("testnet")).toBe(getDeployment());
  });

  it("exposes six contract addresses as non-empty C... strings", () => {
    const { contracts } = getDeployment();
    for (const [name, id] of Object.entries(contracts)) {
      expect(id, name).toMatch(/^C[A-Z0-9]{55}$/);
    }
    expect(Object.keys(contracts).sort()).toEqual(
      [
        "auditorStaking",
        "challengeManager",
        "feeEscrow",
        "registry",
        "reserveVault",
        "usdc",
      ].sort(),
    );
  });

  it("carries network endpoints and deploy provenance", () => {
    const d = getDeployment();
    expect(d.rpcUrl).toMatch(/^https:\/\//);
    expect(d.horizonUrl).toMatch(/^https:\/\//);
    expect(d.networkPassphrase.length).toBeGreaterThan(0);
    expect(d.deployCommit).toMatch(/^[0-9a-f]{40}$/);
    expect(() => new Date(d.deployedAt).toISOString()).not.toThrow();
  });

  it("is JSON-serialisable (no surprises for the browser)", () => {
    expect(() => JSON.stringify(getDeployment())).not.toThrow();
  });
});

describe("listNetworks()", () => {
  it("lists every key in the static map", () => {
    expect(listNetworks()).toEqual(["testnet"]);
  });
});
