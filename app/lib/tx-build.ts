// Server-side transaction builder for the connected-wallet path.
//
// A connected wallet plays an open actor role (auditor / agent / counterparty /
// challenger, plus operator's publish + fee). We build the contract invocation
// here as an *unsigned, already-simulated/assembled* XDR with the wallet as the
// source account, hand it to the browser to sign in its extension, then submit
// the signed envelope. Because each role is BOTH the tx source and the
// `require_auth` address, one envelope signature satisfies Soroban auth — no
// separate auth-entry signing needed.
//
// SERVER ONLY (imports config.ts). No secret keys are used here — the wallet
// signs. The permissionless `resolve` step (challenger flow) is signed
// server-side via BoundClient.resolveChallenge.
import {
  rpc,
  Horizon,
  TransactionBuilder,
  Asset,
  Operation,
  BASE_FEE,
  scValToNative,
  type Transaction,
} from "@stellar/stellar-sdk";
import type { ClientOptions } from "@stellar/stellar-sdk/contract";

import { Client as RegistryClient } from "../../bindings/registry/src";
import { Client as AuditorStakingClient } from "../../bindings/auditor_staking/src";
import { Client as FeeEscrowClient } from "../../bindings/fee_escrow/src";
import { Client as ChallengeManagerClient, type ProofType } from "../../bindings/challenge_manager/src";
import { Client as TokenClient } from "../../bindings/usdc/src";

import { contracts, network, usdc } from "./config";

const G_ADDRESS = /^G[A-Z2-7]{55}$/;
const HORIZON_URL = process.env.STELLAR_HORIZON_URL ?? "https://horizon-testnet.stellar.org";

export type WalletAction = "stake" | "attest" | "publish" | "deposit-fee" | "pay" | "challenge";

export interface BuildParams {
  amountUsd?: number;
  certId?: number;
  agent?: string;
  to?: string;
  victim?: string;
  auditor?: string;
  boundUsd?: number;
  reserveUsd?: number;
  expiryDays?: number;
  bondUsd?: number;
  proofType?: string;
}

function buildOpts(contractId: string, publicKey: string): ClientOptions {
  // No signTransaction → the AssembledTransaction is built + simulated but left
  // unsigned. `.toXDR()` returns the assembled envelope for the wallet to sign.
  return {
    contractId,
    networkPassphrase: network.passphrase,
    rpcUrl: network.rpcUrl,
    publicKey,
  };
}

function requireG(value: string | undefined, name: string): string {
  if (!value || !G_ADDRESS.test(value)) throw new Error(`${name} must be a valid G… address`);
  return value;
}

function requirePositive(amountUsd: number | undefined, name: string): number {
  if (typeof amountUsd !== "number" || !(amountUsd > 0)) throw new Error(`${name} must be a positive amount`);
  return amountUsd;
}

/** Build the unsigned (assembled) XDR for a wallet-played action. */
export async function buildActionXdr(
  action: WalletAction,
  address: string,
  p: BuildParams,
): Promise<string> {
  requireG(address, "wallet address");

  switch (action) {
    case "stake": {
      const c = new AuditorStakingClient(buildOpts(contracts.auditorStaking, address));
      const at = await c.stake({ auditor: address, amount: usdc(requirePositive(p.amountUsd, "stake")) });
      return at.toXDR();
    }
    case "attest": {
      if (p.certId == null) throw new Error("certId required");
      const c = new RegistryClient(buildOpts(contracts.registry, address));
      const at = await c.attest({ auditor: address, cert_id: BigInt(p.certId) });
      return at.toXDR();
    }
    case "publish": {
      const c = new RegistryClient(buildOpts(contracts.registry, address));
      const expiresAt = BigInt(Math.floor(Date.now() / 1000) + (p.expiryDays ?? 30) * 24 * 3600);
      const at = await c.publish({
        operator: address,
        agent: requireG(p.agent, "agent"),
        bound: usdc(p.boundUsd ?? 50_000),
        reserve_amount: usdc(p.reserveUsd ?? 10_000),
        expires_at: expiresAt,
        reserve_vault_contract: contracts.reserveVault,
        auditor_staking_contract: contracts.auditorStaking,
      });
      return at.toXDR();
    }
    case "deposit-fee": {
      const c = new FeeEscrowClient(buildOpts(contracts.feeEscrow, address));
      const at = await c.deposit({
        operator: address,
        auditor: requireG(p.auditor, "auditor"),
        amount: usdc(p.amountUsd ?? 500),
      });
      return at.toXDR();
    }
    case "pay": {
      const c = new TokenClient(buildOpts(contracts.usdc, address));
      const at = await c.transfer({
        from: address,
        to: requireG(p.to, "recipient"),
        amount: usdc(requirePositive(p.amountUsd, "payment")),
      });
      return at.toXDR();
    }
    case "challenge": {
      if (p.certId == null) throw new Error("certId required");
      const c = new ChallengeManagerClient(buildOpts(contracts.challengeManager, address));
      const at = await c.challenge({
        challenger: address,
        cert_id: BigInt(p.certId),
        proof_type: { tag: p.proofType ?? "InsufficientReserve", values: undefined } as ProofType,
        victim: requireG(p.victim, "victim"),
        stake: usdc(p.bondUsd ?? 100),
      });
      return at.toXDR();
    }
    default:
      throw new Error(`unknown action: ${action}`);
  }
}

/**
 * Build an unsigned classic `changeTrust` to the test USDC asset (issued by the
 * operator). A freshly connected wallet signs this to open its USDC trustline
 * before it can hold / stake / bond USDC. Submitted via Horizon (classic).
 */
export async function buildTrustlineXdr(address: string): Promise<string> {
  requireG(address, "wallet address");
  const issuer = requireG(process.env.OPERATOR_ADDRESS, "operator (issuer)");
  const usdcAsset = new Asset("USDC", issuer);

  const horizon = new Horizon.Server(HORIZON_URL);
  const account = await horizon.loadAccount(address);

  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: network.passphrase,
  })
    .addOperation(Operation.changeTrust({ asset: usdcAsset }))
    .setTimeout(180)
    .build();

  return tx.toXDR();
}

/** Submit a wallet-signed envelope, poll to completion, return hash + decoded return value. */
export async function submitSignedXdr(
  signedXdr: string,
): Promise<{ hash: string; result: unknown }> {
  const tx = TransactionBuilder.fromXDR(signedXdr, network.passphrase) as Transaction;

  // Classic (changeTrust, payment) → Horizon; Soroban (contract calls) → RPC.
  const isSoroban = tx.operations.some((op) => op.type === "invokeHostFunction");
  if (!isSoroban) {
    const horizon = new Horizon.Server(HORIZON_URL);
    const resp = await horizon.submitTransaction(tx);
    return { hash: resp.hash, result: undefined };
  }

  const server = new rpc.Server(network.rpcUrl);
  const sent = await server.sendTransaction(tx);
  if (sent.status === "ERROR") {
    throw new Error(`submit rejected: ${JSON.stringify(sent.errorResult ?? sent.status)}`);
  }

  let got = await server.getTransaction(sent.hash);
  const deadline = Date.now() + 30_000;
  while (got.status === "NOT_FOUND" && Date.now() < deadline) {
    await new Promise((r) => setTimeout(r, 1000));
    got = await server.getTransaction(sent.hash);
  }
  if (got.status !== "SUCCESS") {
    throw new Error(`tx ${sent.hash} did not succeed (status ${got.status})`);
  }

  let result: unknown = undefined;
  try {
    if (got.returnValue) {
      const native = scValToNative(got.returnValue);
      result = typeof native === "bigint" ? Number(native) : native;
    }
  } catch {
    /* no decodable return value */
  }
  return { hash: sent.hash, result };
}
