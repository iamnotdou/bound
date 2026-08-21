// Validation-only tests: every case below rejects on argument checks BEFORE any
// RPC call is made, so the suite stays offline.
import { describe, it, expect } from "vitest";
import { buildActionXdr, buildTrustlineXdr } from "./tx";

const G = "GCPOBMCWPO5A24KJJJRD27T4TKITHQI5MYY2FCQRR3HUXUFT4LO473ZT";
const BAD = "not-an-address";
const OTHER = "GCBVHBXWW7CIFKZSGHOZIRYUYIS6S455EW64QK4FXZZ7WVW6R2FZN523";

describe("buildActionXdr() validation", () => {
  it("rejects a wallet address that is not a G… key", async () => {
    await expect(buildActionXdr("stake", BAD, { amountUsd: 10 })).rejects.toThrow(
      /wallet address must be a valid G/,
    );
    await expect(buildActionXdr("stake", "", { amountUsd: 10 })).rejects.toThrow(
      /wallet address must be a valid G/,
    );
  });

  it("rejects a non-positive stake amount", async () => {
    await expect(buildActionXdr("stake", G, { amountUsd: 0 })).rejects.toThrow(
      /stake must be a positive amount/,
    );
    await expect(buildActionXdr("stake", G, { amountUsd: -5 })).rejects.toThrow(
      /stake must be a positive amount/,
    );
    await expect(buildActionXdr("stake", G, {})).rejects.toThrow(/stake must be a positive amount/);
  });

  it("rejects a non-positive payment amount", async () => {
    await expect(buildActionXdr("pay", G, { to: G, amountUsd: 0 })).rejects.toThrow(
      /payment must be a positive amount/,
    );
  });

  it("rejects a bad recipient on pay", async () => {
    await expect(buildActionXdr("pay", G, { to: BAD, amountUsd: 5 })).rejects.toThrow(
      /recipient must be a valid G/,
    );
  });

  it("requires a certId to attest or challenge", async () => {
    await expect(buildActionXdr("attest", G, {})).rejects.toThrow(/certId required/);
    await expect(buildActionXdr("challenge", G, { victim: G })).rejects.toThrow(/certId required/);
  });

  it("rejects a bad agent on publish, auditor on deposit-fee, victim on challenge", async () => {
    await expect(buildActionXdr("publish", G, { agent: BAD })).rejects.toThrow(
      /agent must be a valid G/,
    );
    await expect(buildActionXdr("deposit-fee", G, { auditor: BAD })).rejects.toThrow(
      /auditor must be a valid G/,
    );
    await expect(buildActionXdr("challenge", G, { certId: 1, victim: BAD })).rejects.toThrow(
      /victim must be a valid G/,
    );
  });

  it("rejects an unknown action", async () => {
    await expect(buildActionXdr("teleport" as unknown as "stake", G, {})).rejects.toThrow(
      /unknown action: teleport/,
    );
  });
});

describe("buildTrustlineXdr() validation", () => {
  it("rejects a wallet address that is not a G… key", async () => {
    await expect(buildTrustlineXdr(BAD)).rejects.toThrow(/wallet address must be a valid G/);
  });
  it("refuses to publish for an agent that is not the connected wallet", async () => {
    // v2 authenticates the agent too, and a browser wallet has one signer. The
    // builder refuses rather than producing an envelope the network rejects.
    await expect(buildActionXdr("publish", G, { agent: OTHER })).rejects.toThrow(
      /needs that agent's signature/,
    );
  });
});
