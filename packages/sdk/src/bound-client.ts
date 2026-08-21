// BoundClient — a thin, typed wrapper over the 5 Bound Protocol contracts plus
// the USDC token, built on the Stellar-generated TypeScript bindings.
//
// Reads simulate (no signature). Writes take a Keypair, are signed server-side
// via basicNodeSigner, and submitted with signAndSend().
import { Keypair } from "@stellar/stellar-sdk";
import { basicNodeSigner } from "@stellar/stellar-sdk/contract";
import type { ClientOptions } from "@stellar/stellar-sdk/contract";

import { Client as RegistryClient, type VerifyResult } from "../../../bindings/registry/src";
import { Client as ReserveVaultClient } from "../../../bindings/reserve_vault/src";
import { Client as AuditorStakingClient } from "../../../bindings/auditor_staking/src";
import { Client as FeeEscrowClient } from "../../../bindings/fee_escrow/src";
import {
  Client as ChallengeManagerClient,
  type ProofType,
} from "../../../bindings/challenge_manager/src";
import { Client as TokenClient } from "../../../bindings/usdc/src";
import { Client as PaymentRouterClient } from "../../../bindings/payment_router/src";
import { Client as PremiumVaultClient, type Coverage } from "../../../bindings/premium_vault/src";

import { contracts, network, readSource } from "./config";

export type { VerifyResult, ProofType, Coverage };

/** What a payment did, and — the part that matters — whether it was metered. */
export interface PaymentReceipt {
  hash: string | undefined;
  amount: bigint;
  /** True when the payment moved through the PaymentRouter and hit the meter. */
  routed: boolean;
  /** The certificate the spend was recorded against; null when unrouted. */
  certId: number | null;
}

// Read-only simulations still need a real, funded source account.
const READONLY_SOURCE = readSource;

/**
 * The router and the premium vault are v2 contracts, and `Deployment.contracts`
 * types both as optional so a v1 deployment record still parses. Reaching one
 * on a deployment that predates it should say so, not throw a null-address
 * error from deep inside the bindings.
 */
function required(address: string | undefined, name: string): string {
  if (!address) {
    throw new Error(
      `this deployment has no ${name} address — it predates the v2 contracts. ` +
        `Re-run scripts/deploy-all.ts to get a deployment record with all eight.`,
    );
  }
  return address;
}

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
  private router(signer?: Keypair) {
    return new PaymentRouterClient(
      opts(required(contracts.paymentRouter, "paymentRouter"), signer),
    );
  }
  private premiums(signer?: Keypair) {
    return new PremiumVaultClient(opts(required(contracts.premiumVault, "premiumVault"), signer));
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

  /** Reserve held for one certificate. v2 keys reserves by certificate id — a
   * pooled balance was defect L5, and the single trustless proof read it. */
  async reserveBalance(certId: bigint): Promise<bigint> {
    return (await this.reserve().get_balance({ cert_id: certId })).result;
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
  async depositReserve(operator: Keypair, certId: bigint, amount: bigint) {
    return send(() => this.reserve(operator).deposit({ cert_id: certId, amount }));
  }

  /** Operator escrows the audit fee, naming the auditor who can later collect it. */
  async depositFee(operator: Keypair, auditor: string, amount: bigint) {
    return send(() =>
      this.fees(operator).deposit({ operator: operator.publicKey(), auditor, amount }),
    );
  }

  /** Operator publishes a PENDING certificate. Returns the cert id. */
  /**
   * Publish a certificate. Takes BOTH keypairs because v2's `publish`
   * authenticates the agent as well as the operator.
   *
   * v1 authenticated only the operator and then overwrote the agent's
   * certificate mapping unconditionally, so anyone could publish a junk
   * certificate naming someone else's agent and knock it offline for one
   * transaction (defect L1). Requiring the agent's consent closes that, and
   * matches the router's `enroll`, which already required both.
   *
   * Soroban permits one contract call per transaction, so the two signatures
   * have to land in the same envelope: the operator signs the transaction and
   * the agent signs its authorization entry. A UI cannot split this into two
   * submissions.
   */
  async publishCertificate(
    operator: Keypair,
    agent: Keypair,
    params: { bound: bigint; reserveAmount: bigint; expiresAt: bigint },
  ): Promise<bigint> {
    const at = await this.registry(operator).publish({
      operator: operator.publicKey(),
      agent: agent.publicKey(),
      bound: params.bound,
      reserve_amount: params.reserveAmount,
      expires_at: params.expiresAt,
      reserve_vault_contract: contracts.reserveVault,
      auditor_staking_contract: contracts.auditorStaking,
    });
    const { signAuthEntry } = basicNodeSigner(agent, network.passphrase);
    await at.signAuthEntries({ address: agent.publicKey(), signAuthEntry });
    const sent = await at.signAndSend();
    return sent.result as bigint;
  }

  /** Registered auditor attests → certificate becomes VERIFIED. */
  /** The auditor prices their own risk: `allocation` is the slice of their free
   * stake bonded to this certificate, and the most a slash can ever take. */
  async attestCertificate(auditor: Keypair, certId: bigint, allocation: bigint) {
    return send(() =>
      this.registry(auditor).attest({
        auditor: auditor.publicKey(),
        cert_id: certId,
        allocation,
      }),
    );
  }

  /** Issuer (operator) mints test USDC to a recipient — used to fund a connected wallet. */
  async mintUsdc(issuer: Keypair, to: string, amount: bigint) {
    return send(() => this.token(issuer).mint({ to, amount }));
  }

  /**
   * Send `amount` to `recipient` on the agent's behalf.
   *
   * **If the signer is enrolled in the PaymentRouter the payment goes through
   * the router, and only then is it metered.** The router's `spent(cert_id)`
   * counter is the on-chain state `BoundExceeded` is proven from; a payment
   * that bypasses the router leaves no trace on it, so the covenant it is
   * supposed to evidence would be unprovable. Routing is therefore not an
   * optimisation — it is the thing that makes the second fraud proof mean
   * anything.
   *
   * An unenrolled signer falls back to the raw USDC SAC. That path is honest
   * about what it is: an unmetered payment, by an address no certificate has
   * claimed. `enrollAgent` is what moves an address onto the metered rail.
   *
   * The router holds custody of its own float, so a routed payment spends the
   * signer's *router* balance. When that balance is short, the shortfall is
   * deposited first (one extra transaction) unless `autoFund` is false. Topping
   * up on demand rather than parking a balance is the safer default: the float
   * cap bounds what a stolen agent key can reach, and float that is never idle
   * is float that cannot be stolen.
   */
  async executePayment(
    signer: Keypair,
    recipient: string,
    amount: bigint,
    options: { autoFund?: boolean } = {},
  ): Promise<PaymentReceipt> {
    const certId = contracts.paymentRouter ? await this.routedCertId(signer.publicKey()) : null;

    if (certId === null) {
      const { hash } = await send(() =>
        this.token(signer).transfer({ from: signer.publicKey(), to: recipient, amount }),
      );
      return { hash, amount, routed: false, certId: null };
    }

    if (options.autoFund !== false) {
      const held = await this.routedBalance(signer.publicKey());
      if (held < amount) await this.fundFloat(signer, amount - held);
    }

    const { hash } = await send(() =>
      this.router(signer).transfer({ from: signer.publicKey(), to: recipient, amount }),
    );
    return { hash, amount, routed: true, certId };
  }

  // ---- PaymentRouter: the metered rail ---------------------------------------

  /**
   * Bind `agent` to `certId` and set that certificate's float cap.
   *
   * Both the agent and the operator have to authorize, for opposite reasons —
   * enrollment attaches spend to the operator's certificate, and subjects the
   * agent's address to metering and to the operator's kill switch. Neither
   * party may conscript the other. Like `publishCertificate`, that means two
   * signatures in one envelope: the operator signs the transaction, the agent
   * signs its authorization entry.
   *
   * A binding is permanent. An operator cannot walk an agent off a certificate
   * whose counter is climbing and onto a fresh one — that would make the
   * counter worthless as evidence. Use a new agent address instead.
   */
  async enrollAgent(
    operator: Keypair,
    agent: Keypair,
    certId: bigint,
    floatCap: bigint,
  ): Promise<void> {
    const at = await this.router(operator).enroll({
      agent: agent.publicKey(),
      cert_id: certId,
      float_cap: floatCap,
    });
    const { signAuthEntry } = basicNodeSigner(agent, network.passphrase);
    await at.signAuthEntries({ address: agent.publicKey(), signAuthEntry });
    await at.signAndSend();
  }

  /** Move USDC into the router's custody, crediting an equal routed balance. */
  async fundFloat(holder: Keypair, amount: bigint) {
    return send(() => this.router(holder).deposit({ from: holder.publicKey(), amount }));
  }

  /** Burn routed balance and take the underlying USDC back. */
  async withdrawFloat(holder: Keypair, amount: bigint) {
    return send(() => this.router(holder).withdraw({ to: holder.publicKey(), amount }));
  }

  /** The certificate an address is metered against, or null if it is untracked. */
  async routedCertId(agent: string): Promise<number | null> {
    const id = (await this.router().cert_of({ agent })).result;
    return id === undefined || id === null ? null : Number(id);
  }

  /** An address's balance inside the router (not its USDC balance). */
  async routedBalance(address: string): Promise<bigint> {
    return (await this.router().balance({ id: address })).result;
  }

  /**
   * Cumulative gross flow routed by a certificate — the number
   * `BoundExceeded` is proven against.
   *
   * Gross flow is not loss. A certificate that has routed more than its bound
   * has broken a covenant it made about its own conduct; it has not thereby
   * lost anyone that much money. The contract sizes no payout from this.
   */
  async spendForCert(certId: bigint): Promise<bigint> {
    return (await this.router().spent({ cert_id: certId })).result;
  }

  /** Value the router currently holds on a certificate's behalf. */
  async floatForCert(certId: bigint): Promise<bigint> {
    return (await this.router().float({ cert_id: certId })).result;
  }

  /** The ceiling on that float — what a stolen agent key can reach. */
  async floatCapForCert(certId: bigint): Promise<bigint> {
    return (await this.router().float_cap({ cert_id: certId })).result;
  }

  /** Operator's kill switch: stop every transfer, withdrawal and burn. */
  async haltCert(operator: Keypair, certId: bigint) {
    return send(() => this.router(operator).halt({ cert_id: certId }));
  }

  async resumeCert(operator: Keypair, certId: bigint) {
    return send(() => this.router(operator).resume({ cert_id: certId }));
  }

  async certHalted(certId: bigint): Promise<boolean> {
    return (await this.router().is_halted({ cert_id: certId })).result;
  }

  // ---- PremiumVault: the economy ---------------------------------------------

  /** Price a hypothetical certificate: bound x duration x rate. */
  async quotePremium(bound: bigint, durationSeconds: bigint): Promise<bigint> {
    return (await this.premiums().quote({ bound, duration_seconds: durationSeconds })).result;
  }

  /** Price a published certificate from its own recorded terms. */
  async quotePremiumForCert(certId: bigint): Promise<bigint> {
    return (await this.premiums().quote_cert({ cert_id: certId })).result;
  }

  /**
   * Operator buys coverage. Payable once, and only on a VERIFIED certificate —
   * the premium is yield on the auditor's staked capital, and there is no
   * auditor to pay until one has attested.
   *
   * The protocol's fee share leaves in the same transaction; the rest accrues
   * to the auditor in a straight line across the certificate's term.
   */
  async payPremium(operator: Keypair, certId: bigint) {
    return send(() => this.premiums(operator).pay_premium({ cert_id: certId }));
  }

  /** Yield accrued to the auditor so far, claimed or not. */
  async premiumAccrued(certId: bigint): Promise<bigint> {
    return (await this.premiums().accrued({ cert_id: certId })).result;
  }

  /** What the auditor could withdraw right now: accrued minus already claimed. */
  async premiumClaimable(certId: bigint): Promise<bigint> {
    return (await this.premiums().claimable({ cert_id: certId })).result;
  }

  async premiumPaid(certId: bigint): Promise<boolean> {
    return (await this.premiums().is_paid({ cert_id: certId })).result;
  }

  /** The full coverage record: premium, auditor, term, accrual start, claimed. */
  async coverage(certId: bigint): Promise<Coverage> {
    return (await this.premiums().get_coverage({ cert_id: certId })).result;
  }

  /**
   * Auditor withdraws accrued-and-unclaimed yield. Returns the amount moved.
   *
   * A slash forfeits only what is *unclaimed*, so an auditor who claims
   * continuously carries less forfeitable yield than one who lets it pile up.
   * That is deliberate: the premium is yield on the stake, not a second bond,
   * and treating unclaimed yield as collateral would overstate the cover.
   */
  async claimPremium(
    auditor: Keypair,
    certId: bigint,
  ): Promise<{ hash: string | undefined; result: bigint }> {
    return send<bigint>(() => this.premiums(auditor).claim({ cert_id: certId }));
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
    // v2 does not settle on filing. The challenge opens (or joins) a 72-hour
    // claim window; settlement happens once, over every admitted claim, when
    // `close_window` is called after it lapses.
    return challengeId;
  }

  /**
   * Close a certificate's claim window once it has lapsed, settling every
   * admitted claim at once. Permissionless — any funded signer may call it, and
   * nobody is paid for doing so; the incentive is that no claimant is paid until
   * someone does. Replaces v1's per-challenge `resolve`, which settled the first
   * claim and foreclosed every honest one behind it.
   */
  async closeClaimWindow(signer: Keypair, certId: bigint) {
    return send(() => this.challenges(signer).close_window({ cert_id: certId }));
  }

  /** When a certificate's open claim window may be closed. 0 if none is open. */
  async windowClosesAt(certId: bigint): Promise<bigint> {
    return (await this.challenges().window_closes_at({ cert_id: certId })).result;
  }

  /** True once a certificate's window has been closed and settled. */
  async claimWindowSettled(certId: bigint): Promise<boolean> {
    return (await this.challenges().is_settled({ cert_id: certId })).result;
  }

  /** How long a claim window stays open, in seconds. */
  async claimWindowSeconds(): Promise<bigint> {
    return (await this.challenges().get_claim_window_seconds()).result;
  }

  // ---- cheat simulations (read-only) ----------------------------------------
  // These build + simulate a defection WITHOUT signing/submitting. If the
  // contract's lock holds, simulation traps and the promise rejects — that
  // rejection IS the proof. No state changes, no funds spent. Used by the
  // /control adversarial lane to show "the cage holds" on screen.

  /** Operator tries to reclaim the reserve before expiry → expect `reserve_still_locked`. */
  async simulateReleaseReserve(operator: Keypair, certId: bigint): Promise<void> {
    const tx = await this.reserve(operator).release_to_operator({ cert_id: certId });
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
