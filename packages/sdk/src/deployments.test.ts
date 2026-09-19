import { describe, it, expect } from "vitest";
import {
  DEFAULT_NETWORK,
  NETWORK_NAMES,
  getDeployment,
  isNetworkName,
  listNetworks,
  parseNetwork,
  serializeDeployment,
} from "./deployments";
import type { Deployment } from "./deployments";

describe("getDeployment()", () => {
  it("defaults to testnet", () => {
    expect(getDeployment().network).toBe("testnet");
  });

  it("returns the same object for an explicit testnet lookup", () => {
    expect(getDeployment("testnet")).toBe(getDeployment());
  });

  it("exposes every contract address as non-empty C... strings", () => {
    const { contracts } = getDeployment();
    for (const [name, id] of Object.entries(contracts)) {
      expect(id, name).toMatch(/^C[A-Z0-9]{55}$/);
    }
    expect(Object.keys(contracts).sort()).toEqual(
      [
        "auditorStaking",
        "challengeManager",
        "feeEscrow",
        "paymentRouter",
        "premiumVault",
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

  it("exposes five actor public keys and no secrets", () => {
    const { accounts } = getDeployment();
    expect(Object.keys(accounts).sort()).toEqual([
      "agent",
      "auditor",
      "challenger",
      "counterparty",
      "operator",
    ]);
    for (const [role, address] of Object.entries(accounts)) {
      // G... only. An S... here would be a leaked secret key in a public repo.
      expect(address, role).toMatch(/^G[A-Z0-9]{55}$/);
    }
  });

  it("contains no S... secret key anywhere in the committed file", () => {
    // Cheap guard against a deploy script one day writing the wrong field.
    expect(JSON.stringify(getDeployment())).not.toMatch(/"S[A-Z2-7]{55}"/);
  });

  it("is JSON-serialisable (no surprises for the browser)", () => {
    expect(() => JSON.stringify(getDeployment())).not.toThrow();
  });
});

describe("listNetworks()", () => {
  it("lists every name in NETWORK_NAMES", () => {
    expect(listNetworks()).toEqual([...NETWORK_NAMES]);
  });

  it("returns a fresh array a caller cannot use to mutate the set", () => {
    const first = listNetworks();
    first.push("nope" as never);
    expect(listNetworks()).toEqual([...NETWORK_NAMES]);
  });

  it("names a deployment for every network, and no orphans", () => {
    // The guard that makes adding a network safe: the tuple and the map cannot
    // drift apart without this failing.
    for (const name of listNetworks()) {
      expect(getDeployment(name).network, name).toBe(name);
    }
  });
});

describe("DEFAULT_NETWORK", () => {
  it("is one of the networks that actually exist", () => {
    expect(listNetworks()).toContain(DEFAULT_NETWORK);
  });

  it("is what getDeployment() returns with no argument", () => {
    expect(getDeployment()).toBe(getDeployment(DEFAULT_NETWORK));
  });
});

describe("isNetworkName()", () => {
  it("accepts every name the package carries", () => {
    for (const name of NETWORK_NAMES) expect(isNetworkName(name)).toBe(true);
  });

  it("rejects an unknown name", () => {
    expect(isNetworkName("mainnet")).toBe(false);
    expect(isNetworkName("testnet-anchor")).toBe(false);
  });

  it("rejects non-strings rather than throwing on them", () => {
    for (const value of [undefined, null, 0, {}, [], true]) {
      expect(isNetworkName(value)).toBe(false);
    }
  });

  it("does not accept a name inherited from Object.prototype", () => {
    // `includes` on the tuple, not a key lookup on an object — so "toString"
    // and friends are not networks.
    expect(isNetworkName("toString")).toBe(false);
    expect(isNetworkName("constructor")).toBe(false);
  });
});

describe("parseNetwork()", () => {
  it("treats unspecified as the default", () => {
    expect(parseNetwork(undefined)).toBe(DEFAULT_NETWORK);
    expect(parseNetwork(null)).toBe(DEFAULT_NETWORK);
    expect(parseNetwork("")).toBe(DEFAULT_NETWORK);
    // A variable set to whitespace is a variable nobody meant to set.
    expect(parseNetwork("   ")).toBe(DEFAULT_NETWORK);
  });

  it("accepts a known name, trimmed", () => {
    expect(parseNetwork("testnet")).toBe("testnet");
    expect(parseNetwork("  testnet  ")).toBe("testnet");
  });

  it("refuses an unknown name and says which ones exist", () => {
    // The message has to carry the known set: the whole failure mode this
    // guards is somebody pointing at a deployment that was never committed.
    expect(() => parseNetwork("testnet-anchor")).toThrow(/testnet-anchor/);
    expect(() => parseNetwork("testnet-anchor")).toThrow(/known: testnet/);
  });

  it("is case-sensitive — a network name is an exact key", () => {
    expect(() => parseNetwork("TESTNET")).toThrow();
  });

  it("reads no environment of its own", () => {
    // It is exported from the browser-safe subpath, so it must stay pure.
    const previous = process.env.STELLAR_NETWORK;
    process.env.STELLAR_NETWORK = "testnet-anchor";
    try {
      expect(parseNetwork(undefined)).toBe(DEFAULT_NETWORK);
    } finally {
      if (previous === undefined) delete process.env.STELLAR_NETWORK;
      else process.env.STELLAR_NETWORK = previous;
    }
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
    accounts: {
      operator: "G" + "A".repeat(55),
      agent: "G" + "G".repeat(55),
      auditor: "G" + "H".repeat(55),
      challenger: "G" + "I".repeat(55),
      counterparty: "G" + "J".repeat(55),
    },
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
  "accounts": {
    "operator": "G${"A".repeat(55)}",
    "agent": "G${"G".repeat(55)}",
    "auditor": "G${"H".repeat(55)}",
    "challenger": "G${"I".repeat(55)}",
    "counterparty": "G${"J".repeat(55)}"
  },
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
