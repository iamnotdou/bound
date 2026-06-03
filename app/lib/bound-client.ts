// BoundClient — a thin, typed wrapper over the 5 Bound Protocol contracts plus
// the USDC token, built on the Stellar-generated TypeScript bindings.
//
// Reads simulate (no signature). Writes take a Keypair, are signed server-side
// via basicNodeSigner, and submitted with signAndSend().
import { Keypair } from "@stellar/stellar-sdk";
import { basicNodeSigner } from "@stellar/stellar-sdk/contract";
import type { ClientOptions } from "@stellar/stellar-sdk/contract";

import { Client as RegistryClient, type VerifyResult } from "../../bindings/registry/src";
import { Client as ReserveVaultClient } from "../../bindings/reserve_vault/src";
import { Client as AuditorStakingClient } from "../../bindings/auditor_staking/src";
import { Client as FeeEscrowClient } from "../../bindings/fee_escrow/src";
import { Client as ChallengeManagerClient, type ProofType } from "../../bindings/challenge_manager/src";
import { Client as TokenClient } from "../../bindings/usdc/src";

import { contracts, network, readSource } from "./config";

export type { VerifyResult, ProofType };

// Read-only simulations still need a real, funded source account.
const READONLY_SOURCE = readSource;

function opts(contractId: string, signer?: Keypair): ClientOptions {
  const base: ClientOptions = {
    contractId,
    networkPassphrase: network.passphrase,
    rpcUrl: network.rpcUrl,
    publicKey: signer?.publicKey() ?? READONLY_SOURCE,
  };
  if (!signer) return base;
  const { signTransaction } = basicNodeSigner(signer, network.passphrase);
  return { ...base, signTransaction };
}

/** Run a write: assemble, sign with `signer`, submit, return the tx hash + result. */
async function send<T>(
  build: () => Promise<{ signAndSend: () => Promise<any> }>,
): Promise<{ hash: string | undefined; result: T }> {
  const at = await build();
  const sent = await at.signAndSend();
  return { hash: sent.sendTransactionResponse?.hash, result: sent.result as T };
}

export class BoundClient {
  private registry(signer?: Keypair) {
    return new RegistryClient(opts(contracts.registry, signer));
  }
  private reserve(signer?: Keypair) {
    return new ReserveVaultClient(opts(contracts.reserveVault, signer));
  }
  private staking(signer?: Keypair) {
    return new AuditorStakingClient(opts(contracts.auditorStaking, signer));
  }
  private fees(signer?: Keypair) {
    return new FeeEscrowClient(opts(contracts.feeEscrow, signer));
  }
  private challenges(signer?: Keypair) {
    return new ChallengeManagerClient(opts(contracts.challengeManager, signer));
  }
  private token(signer?: Keypair) {
    return new TokenClient(opts(contracts.usdc, signer));
  }

  // ---- reads ----------------------------------------------------------------

  /** The headline check a counterparty runs before transacting. */
  async verifyCertificate(agent: string): Promise<VerifyResult> {
    const at = await this.registry().verify({ agent });
    return at.result;
  }

  /**
   * The cert id mapped to an agent, or null if none. The challenge flow needs
   * this and `verify()` doesn't return it, so the UI reads it here.
   */
  async certIdForAgent(agent: string): Promise<number | null> {
    try {
      const id = (await this.registry().get_cert_id({ agent })).result;
      return id ? Number(id) : null;
    } catch {
      return null; // no cert mapped to this agent yet
    }
  }

  async usdcBalance(address: string): Promise<bigint> {
    return (await this.token().balance({ id: address })).result;
  }

  async reserveBalance(): Promise<bigint> {
    return (await this.reserve().get_balance()).result;
  }

  async auditorStake(auditor: string): Promise<bigint> {
    return (await this.staking().get_stake({ auditor })).result;
  }

  /** True once the auditor's locked stake meets the registration minimum. */
  async auditorRegistered(auditor: string): Promise<boolean> {
    return (await this.staking().is_registered({ auditor })).result;
  }

  /** The minimum stake an auditor must lock to be a registered (and able to attest). */
  async auditorMinStake(): Promise<bigint> {
    return (await this.staking().get_min_stake()).result;
  }

  // ---- writes ---------------------------------------------------------------

  /** Auditor locks their own slashable capital. */
  async stakeAsAuditor(auditor: Keypair, amount: bigint) {
    return send(() => this.staking(auditor).stake({ auditor: auditor.publicKey(), amount }));
  }

  /** Operator funds the reserve. */
  async depositReserve(operator: Keypair, amount: bigint) {
    return send(() => this.reserve(operator).deposit({ amount }));
  }

  /** Operator escrows the audit fee, naming the auditor who can later collect it. */
  async depositFee(operator: Keypair, auditor: string, amount: bigint) {
    return send(() => this.fees(operator).deposit({ operator: operator.publicKey(), auditor, amount }));
  }

  /** Operator publishes a PENDING certificate. Returns the cert id. */
  async publishCertificate(
    operator: Keypair,
    params: { agent: string; bound: bigint; reserveAmount: bigint; expiresAt: bigint },
  ): Promise<bigint> {
    const { result } = await send<bigint>(() =>
      this.registry(operator).publish({
        operator: operator.publicKey(),
        agent: params.agent,
        bound: params.bound,
        reserve_amount: params.reserveAmount,
        expires_at: params.expiresAt,
        reserve_vault_contract: contracts.reserveVault,
        auditor_staking_contract: contracts.auditorStaking,
      }),
    );
    return result;
  }

  /** Registered auditor attests → certificate becomes VERIFIED. */
  async attestCertificate(auditor: Keypair, certId: bigint) {
    return send(() => this.registry(auditor).attest({ auditor: auditor.publicKey(), cert_id: certId }));
  }

  /** Issuer (operator) mints test USDC to a recipient — used to fund a connected wallet. */
  async mintUsdc(issuer: Keypair, to: string, amount: bigint) {
    return send(() => this.token(issuer).mint({ to, amount }));
  }

  /** Agent (or anyone) sends USDC directly to a recipient. Returns the tx hash. */
  async executePayment(signer: Keypair, recipient: string, amount: bigint): Promise<string | undefined> {
    const { hash } = await send(() =>
      this.token(signer).transfer({ from: signer.publicKey(), to: recipient, amount }),
    );
    return hash;
  }

  /**
   * Open a challenge and resolve it. For InsufficientReserve the contract proves
   * the fraud itself on-chain — slashing the auditor and compensating the victim.
   * Returns the challenge id.
   */
  async challengeCertificate(
    challenger: Keypair,
    params: { certId: bigint; proofType: ProofType["tag"]; victim: string; bond: bigint },
  ): Promise<bigint> {
    const cm = this.challenges(challenger);
    const { result: challengeId } = await send<bigint>(() =>
      cm.challenge({
        challenger: challenger.publicKey(),
        cert_id: params.certId,
        proof_type: { tag: params.proofType, values: undefined } as ProofType,
        victim: params.victim,
        stake: params.bond,
      }),
    );
    await send(() => cm.resolve({ challenge_id: challengeId }));
    return challengeId;
  }

  /**
   * Resolve a challenge that was already opened (e.g. by a connected wallet that
   * signed `challenge` itself). `resolve` is permissionless — any funded signer
   * can trigger the on-chain proof; the finder's fee still routes to the
   * challenger recorded on the challenge. Used by the wallet challenger flow.
   */
  async resolveChallenge(signer: Keypair, challengeId: bigint) {
    return send(() => this.challenges(signer).resolve({ challenge_id: challengeId }));
  }

  // ---- cheat simulations (read-only) ----------------------------------------
  // These build + simulate a defection WITHOUT signing/submitting. If the
  // contract's lock holds, simulation traps and the promise rejects — that
  // rejection IS the proof. No state changes, no funds spent. Used by the
  // /control adversarial lane to show "the cage holds" on screen.

  /** Operator tries to reclaim the reserve before expiry → expect `reserve_still_locked`. */
  async simulateReleaseReserve(operator: Keypair): Promise<void> {
    const tx = await this.reserve(operator).release_to_operator();
    // With a signer configured the client won't reject on a failed simulation;
    // reading `.result` forces the verdict so a trapped lock surfaces as a throw.
    void (tx as { result: unknown }).result;
  }

  /** Auditor tries to withdraw a stake bonded to a live cert → expect `stake_locked`. */
  async simulateReleaseStake(auditor: Keypair): Promise<void> {
    const tx = await this.staking(auditor).release({ auditor: auditor.publicKey() });
    void (tx as { result: unknown }).result;
  }
}

export const bound = new BoundClient();
