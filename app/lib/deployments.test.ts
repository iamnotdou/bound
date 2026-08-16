import { describe, it, expect } from "vitest";
import { getDeployment, listNetworks, serializeDeployment } from "./deployments";
import type { Deployment } from "./deployments";

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

  it("carries network endpoints, readSource, and deploy provenance", () => {
    const d = getDeployment();
    expect(d.rpcUrl).toMatch(/^https:\/\//);
    expect(d.horizonUrl).toMatch(/^https:\/\//);
    expect(d.networkPassphrase.length).toBeGreaterThan(0);
    expect(d.readSource).toMatch(/^G[A-Z0-9]{55}$/);
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

describe("serializeDeployment()", () => {
  // Fake inputs — deliberately not the live addresses, so the test is about
  // shape, not about the current deployment.
  const input: Deployment = {
    network: "testnet",
    networkPassphrase: "Test SDF Network ; September 2015",
    rpcUrl: "https://rpc.example.test",
    horizonUrl: "https://horizon.example.test",
    deployedAt: "2026-01-02T03:04:05.000Z",
    deployCommit: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    readSource: "G" + "A".repeat(55),
    contracts: {
      registry: "C" + "A".repeat(55),
      reserveVault: "C" + "B".repeat(55),
      auditorStaking: "C" + "C".repeat(55),
      feeEscrow: "C" + "D".repeat(55),
      challengeManager: "C" + "E".repeat(55),
      usdc: "C" + "F".repeat(55),
    },
  };

  // Exact expected string. If this drifts, the on-disk format drifted — that is
  // the whole point of locking the serialiser down with a pure function.
  const expected = `{
  "network": "testnet",
  "networkPassphrase": "Test SDF Network ; September 2015",
  "rpcUrl": "https://rpc.example.test",
  "horizonUrl": "https://horizon.example.test",
  "deployedAt": "2026-01-02T03:04:05.000Z",
  "deployCommit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "readSource": "G${"A".repeat(55)}",
  "contracts": {
    "registry": "C${"A".repeat(55)}",
    "reserveVault": "C${"B".repeat(55)}",
    "auditorStaking": "C${"C".repeat(55)}",
    "feeEscrow": "C${"D".repeat(55)}",
    "challengeManager": "C${"E".repeat(55)}",
    "usdc": "C${"F".repeat(55)}"
  }
}
`;

  it("emits the exact canonical JSON shape", () => {
    expect(serializeDeployment(input)).toBe(expected);
  });

  it("is a no-op round-trip against the committed testnet file", () => {
    // The live file was written by hand at 3.1; from 3.2 the deploy script
    // owns it. Either way, re-serialising what we load must not churn the file.
    const live = getDeployment("testnet");
    expect(serializeDeployment(live)).toBe(
      serializeDeployment(JSON.parse(serializeDeployment(live))),
    );
    // And the serialised form must parse back to the same values.
    expect(JSON.parse(serializeDeployment(live))).toEqual(live);
  });

  it("drops extra properties a caller might pass", () => {
    const dirty = { ...input, extra: "nope", contracts: { ...input.contracts, other: "x" } };
    const parsed = JSON.parse(serializeDeployment(dirty as Deployment));
    expect(parsed.extra).toBeUndefined();
    expect(parsed.contracts.other).toBeUndefined();
    expect(Object.keys(parsed.contracts).sort()).toEqual(
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

  it("ends with a trailing newline", () => {
    expect(serializeDeployment(input).endsWith("\n")).toBe(true);
  });
});
