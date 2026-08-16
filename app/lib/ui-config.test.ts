import { describe, it, expect } from "vitest";
import { roles, contracts, network, NETWORK, truncate, roleForAddress } from "./ui-config";
import { getDeployment } from "./deployments";

const deployment = getDeployment();

describe("ui-config (client-safe)", () => {
  it("resolves every role address from the committed deployment", () => {
    // Before 3.4 these came from next.config's build-time env inlining and were
    // "" whenever the build ran without .env.testnet — a silently empty UI.
    expect(roles.operator.address).toBe(deployment.accounts.operator);
    expect(roles.agent.address).toBe(deployment.accounts.agent);
    expect(roles.auditor.address).toBe(deployment.accounts.auditor);
    expect(roles.challenger.address).toBe(deployment.accounts.challenger);
    expect(roles.counterparty.address).toBe(deployment.accounts.counterparty);
  });

  it("has no empty address anywhere", () => {
    for (const [key, role] of Object.entries(roles)) {
      expect(role.address, key).toMatch(/^G[A-Z0-9]{55}$/);
    }
    for (const [key, contract] of Object.entries(contracts)) {
      expect(contract.id, key).toMatch(/^C[A-Z0-9]{55}$/);
    }
  });

  it("exposes the deployment's network endpoints", () => {
    expect(NETWORK).toBe("testnet");
    expect(network.rpcUrl).toBe(deployment.rpcUrl);
    expect(network.horizonUrl).toBe(deployment.horizonUrl);
    expect(network.passphrase).toBe(deployment.networkPassphrase);
  });

  it("never exposes a secret key", () => {
    const blob = JSON.stringify({ roles, contracts, network });
    expect(blob).not.toMatch(/"S[A-Z2-7]{55}"/);
  });
});

describe("truncate()", () => {
  it("shortens long addresses around an ellipsis", () => {
    expect(truncate("GABCDEFGHIJKLMNOP")).toBe("GABC…MNOP");
  });

  it("leaves short values alone", () => {
    expect(truncate("GABC")).toBe("GABC");
    expect(truncate("")).toBe("");
  });

  it("honours custom head/tail widths", () => {
    expect(truncate("GABCDEFGHIJKLMNOP", 2, 2)).toBe("GA…OP");
  });
});

describe("roleForAddress()", () => {
  it("maps a known address back to its label", () => {
    expect(roleForAddress(deployment.accounts.agent)).toBe("Agent");
    expect(roleForAddress(deployment.accounts.operator)).toBe("Operator");
  });

  it("returns null for an unknown address", () => {
    expect(roleForAddress("G" + "Z".repeat(55))).toBeNull();
  });
});
