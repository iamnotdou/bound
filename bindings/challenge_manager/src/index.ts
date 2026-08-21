import { Buffer } from "buffer";
import { Address } from "@stellar/stellar-sdk";
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from "@stellar/stellar-sdk/contract";
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  Option,
  Timepoint,
  Duration,
} from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";

if (typeof window !== "undefined") {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}


export const networks = {
  testnet: {
    networkPassphrase: "Test SDF Network ; September 2015",
    contractId: "CAYEGPIHNDIEONWNKRF2UPTO32SXGFTLBQ2K4RPN2LCIGOOZYLYYYIHY",
  }
} as const

export type DataKey = {tag: "Registry", values: void} | {tag: "AuditorStaking", values: void} | {tag: "ReserveVault", values: void} | {tag: "FeeEscrow", values: void} | {tag: "Token", values: void} | {tag: "Arbiter", values: void} | {tag: "Router", values: void} | {tag: "PremiumVault", values: void} | {tag: "Treasury", values: void} | {tag: "MinStake", values: void} | {tag: "Challenge", values: readonly [u64]} | {tag: "ChallengeCount", values: void} | {tag: "BondsHeld", values: void} | {tag: "Window", values: readonly [u64]} | {tag: "Settled", values: readonly [u64]};

export type Verdict = {tag: "Pending", values: void} | {tag: "ChallengeWins", values: void} | {tag: "ChallengeFails", values: void} | {tag: "Cured", values: void} | {tag: "Unadjudicated", values: void};


export interface Challenge {
  /**
 * False only for a `FakeSignature` claim the arbiter has not ruled on yet.
 * Every on-chain predicate adjudicates itself at filing.
 */
adjudicated: boolean;
  /**
 * True when `harm` was **stated by the arbiter** rather than computed by a
 * predicate. The distinction decides how the number aggregates across a
 * claim window — see `close_window`.
 */
arbitrated: boolean;
  cert_id: u64;
  challenger: string;
  /**
 * Ledger time the claim was filed. DESIGN-V2 §2: the predicate is
 * evaluated here, and this is the instant the recorded state belongs to.
 */
filed_at: u64;
  /**
 * The harm the predicate quantified **at filing**, in stroops. This is the
 * number the pro-rata settlement divides by; live state never resizes it.
 */
harm: i128;
  proof_type: ProofType;
  /**
 * DESIGN-V2 §2. The predicate's value **at filing**, recorded once and
 * never recomputed. What was true when the challenge was filed stays true,
 * so an operator cannot flip it to false and pocket the bond.
 */
proven: boolean;
  stake: i128;
  verdict: Verdict;
  victim: string;
}

export type ProofType = {tag: "InsufficientReserve", values: void} | {tag: "BoundExceeded", values: void} | {tag: "ExpiredCertificate", values: void} | {tag: "FakeSignature", values: void};


/**
 * A structural mirror of `payment_router::PostExpiry`.
 * 
 * The ChallengeManager reads the router over `invoke_contract`, so it needs the
 * return type locally. A `#[contracttype]` struct is encoded as a map keyed by
 * field **name**, so this decodes the router's value as long as the names and
 * types match. It is duplicated rather than imported to keep the router out of
 * this crate's dependency graph — the contracts are deployed and upgraded
 * independently, and a compile-time dependency would not make the on-chain ABI
 * any more coupled than it already is. If you change `PostExpiry` in the
 * router, change it here; `expired_certificate_upholds_when_all_three_conditions_hold`
 * in the integration harness fails loudly if the two drift.
 */
export interface PostExpiry {
  count: u32;
  first_at: u64;
  max_payment: i128;
  max_payment_at: u64;
  total: i128;
}


/**
 * DESIGN-V2 §1. The claim window opened by the first valid challenge against a
 * certificate.
 * 
 * A window is a *certificate-level* object, not a challenge-level one. That is
 * the whole point: settlement runs once, over every claim the window admitted,
 * so being first is worth nothing.
 */
export interface ClaimWindow {
  cert_id: u64;
  /**
 * Every claim filed against this certificate while the window was open,
 * in filing order. The order is recorded for auditability only — no payout
 * anywhere below reads it.
 */
claims: Array<u64>;
  closes_at: u64;
  opened_at: u64;
}

export interface Client {
  /**
   * Construct and simulate a challenge transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * File a claim against a certificate. Anyone may; a bond keeps it honest.
   * 
   * DESIGN-V2 §1 + §2. This call does three things that the pre-window
   * lifecycle did not:
   * 
   * 1. **It evaluates the predicate now, and records the answer.** Nothing
   * downstream ever recomputes whether the challenger was right — only
   * whether the operator has since fixed it. An operator who tops the
   * reserve back up during the window can no longer flip the predicate to
   * false and pocket the bond.
   * 2. **A claim that is true at filing opens or joins a claim window**
   * rather than settling. The first one freezes the certificate; the rest
   * queue up behind it and are all paid together, pro rata, at close.
   * 3. **A claim that is false at filing, with no window open, is rejected
   * on the spot** and its bond forfeited. It deliberately does not open a
   * window: if a wrong claim could freeze a certificate for 72 hours,
   * anybody could freeze any certificate for the price of the minimum
   * bond. Once a window *is* open a wrong claim is allowed to join it —
   * it changes n
   */
  challenge: ({challenger, cert_id, proof_type, victim, stake}: {challenger: string, cert_id: u64, proof_type: ProofType, victim: string, stake: i128}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_router transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_router: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_window transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The claim window, if one is open on this certificate.
   */
  get_window: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Option<ClaimWindow>>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({registry, auditor_staking, reserve_vault, fee_escrow, token, arbiter, treasury, min_stake}: {registry: string, auditor_staking: string, reserve_vault: string, fee_escrow: string, token: string, arbiter: string, treasury: string, min_stake: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a is_settled transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Whether this certificate has already been settled through a closed
   * window. A settled certificate accepts no further claims.
   */
  is_settled: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a set_router transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Point this contract at the PaymentRouter.
   * 
   * Separate from `initialize` for one reason: the argument list of a
   * `pub fn` in a `#[contractimpl]` block is the contract's ABI, and
   * `initialize` already has eight parameters that the deploy script and the
   * committed bindings pass positionally. A ninth would break both at
   * runtime for no gain, so the wiring is a second one-shot call instead.
   * 
   * **Arbiter-authorized, and settable exactly once.** `initialize` is
   * unauthenticated (a known defect, tested as such), and copying that here
   * would be materially worse: whoever wins the race to name the router names
   * the contract that reports `spent`, and a lying router can invalidate any
   * certificate it likes. The arbiter is already a named trusted party at
   * `initialize`, so requiring their signature grants no new trust and closes
   * the race. There is no re-pointing, for the same reason the treasury
   * cannot be re-pointed.
   */
  set_router: ({router}: {router: string}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a close_window transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  close_window: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_treasury transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_treasury: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_challenge transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_challenge: ({challenge_id}: {challenge_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Challenge>>

  /**
   * Construct and simulate a get_bonds_held transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_bonds_held: (options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_bounty_pool transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Forfeited bonds the contract holds beyond what it still owes back. This
   * is the only pot the hygiene bounty is paid from.
   */
  get_bounty_pool: (options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a window_closes_at transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Ledger time at which this certificate's open window may be closed.
   * `0` means no window is open.
   */
  window_closes_at: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_premium_vault transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_premium_vault: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_reserve_vault transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The vault every reserve proof and every settlement payment reads.
   * 
   * Exposed for the Registry, which needs it at `attest` (DESIGN-V2 §4) to
   * check a certificate's reserve is funded *against the same contract this
   * one will later measure the shortfall on*. Publishing the address rather
   * than letting the Registry hold its own copy is what makes the two
   * checks agree by construction instead of by convention: there is exactly
   * one `DataKey::ReserveVault` in the deployment, and it is this one.
   */
  get_reserve_vault: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a has_premium_vault transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Whether step 4 is live. False means a deployment that never called
   * `set_premium_vault`, in which case settlement silently skips the premium
   * step — see the comment on step 4 in `settle_fraud`.
   */
  has_premium_vault: (options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a set_premium_vault transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Point this contract at the PremiumVault, which owns step 4 of the
   * settlement waterfall.
   * 
   * A second one-shot call rather than a tenth `initialize` argument, for
   * exactly the reasons spelled out on `set_router`: `initialize`'s argument
   * list is the on-chain ABI, the deploy script and the committed bindings
   * pass it positionally, and widening it would break both at runtime.
   * 
   * **Arbiter-authorized, and settable exactly once.** The vault named here
   * is handed the certificate's forfeited premium and told where to send it,
   * so whoever names it can name a contract that keeps the money. The arbiter
   * is already a trusted party at `initialize`, so requiring their signature
   * grants no new trust and closes the race. There is no re-pointing, for the
   * same reason the treasury cannot be re-pointed.
   */
  set_premium_vault: ({premium_vault}: {premium_vault: string}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a close_window_early transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * DESIGN-V2 §10. Close a window early, on the arbiter's rejection of the
   * only thing keeping it open.
   * 
   * ## The abuse this closes
   * 
   * `FakeSignature` has **no predicate**. A claim carrying it cannot be
   * evaluated at filing the way a false `InsufficientReserve` claim is, so
   * it cannot be rejected on the spot — it opens a window and waits for the
   * arbiter. It used to freeze the certificate for those 72 hours as well:
   * no attestation, no reserve withdrawal, no allocation release. Anyone
   * could buy that freeze against any certificate for the price of the
   * minimum bond, and an arbiter ruling the claim false did nothing to
   * shorten it.
   * 
   * **R3 has since removed the freeze from arbiter-gated claims entirely**,
   * so the abuse this call was built for no longer has anything to buy. This
   * call is not thereby redundant: an open window still blocks a second
   * window on the same certificate and still holds the claimants' bonds, and
   * ending it on the arbiter's rejection is the right outcome rather than
   * merely the cheap one.
   * 
   * The fix is to make
   */
  close_window_early: ({challenge_id}: {challenge_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a resolve_by_arbiter transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Arbiter adjudication: a verdict and a **quantity** on one live claim.
   * This is an explicit trust assumption; the arbiter is named at
   * `initialize`.
   * 
   * It reaches any claim inside an open window, not just `FakeSignature`.
   * That is what lets `BoundExceeded` and `ExpiredCertificate` — whose
   * on-chain predicates are true but whose counters are never a loss — be
   * given an assessed harm and settle through the full waterfall instead of
   * hygiene mode.
   * 
   * WHAT IT CANNOT REACH — **R4**, and this is DESIGN-V2 §2 working in both
   * directions. On a claim carrying an on-chain predicate the arbiter may
   * ADD to what the contract proved and may never CONTRADICT it: the verdict
   * must match what the predicate recorded at filing, and the harm may not
   * fall below the number it computed. The router and the vault are the
   * source of truth for what they measure — no human may declare a breach
   * they say did not happen, and none may deny one that did. The rule is
   * enforced in the body, where the reasoning is written out in full.
   * 
   * Only `FakeSign
   */
  resolve_by_arbiter: ({challenge_id, fraud_proven, harm}: {challenge_id: u64, fraud_proven: boolean, harm: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_challenge_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_challenge_count: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_claim_window_seconds transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * How long a claim window stays open, in ledger seconds.
   */
  get_claim_window_seconds: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

}
export class Client extends ContractClient {
  static async deploy<T = Client>(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions &
      Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
      }
  ): Promise<AssembledTransaction<T>> {
    return ContractClient.deploy(null, options)
  }
  constructor(public readonly options: ContractClientOptions) {
    super(
      new ContractSpec([ "AAAAAAAABABGaWxlIGEgY2xhaW0gYWdhaW5zdCBhIGNlcnRpZmljYXRlLiBBbnlvbmUgbWF5OyBhIGJvbmQga2VlcHMgaXQgaG9uZXN0LgoKREVTSUdOLVYyIMKnMSArIMKnMi4gVGhpcyBjYWxsIGRvZXMgdGhyZWUgdGhpbmdzIHRoYXQgdGhlIHByZS13aW5kb3cKbGlmZWN5Y2xlIGRpZCBub3Q6CgoxLiAqKkl0IGV2YWx1YXRlcyB0aGUgcHJlZGljYXRlIG5vdywgYW5kIHJlY29yZHMgdGhlIGFuc3dlci4qKiBOb3RoaW5nCmRvd25zdHJlYW0gZXZlciByZWNvbXB1dGVzIHdoZXRoZXIgdGhlIGNoYWxsZW5nZXIgd2FzIHJpZ2h0IOKAlCBvbmx5CndoZXRoZXIgdGhlIG9wZXJhdG9yIGhhcyBzaW5jZSBmaXhlZCBpdC4gQW4gb3BlcmF0b3Igd2hvIHRvcHMgdGhlCnJlc2VydmUgYmFjayB1cCBkdXJpbmcgdGhlIHdpbmRvdyBjYW4gbm8gbG9uZ2VyIGZsaXAgdGhlIHByZWRpY2F0ZSB0bwpmYWxzZSBhbmQgcG9ja2V0IHRoZSBib25kLgoyLiAqKkEgY2xhaW0gdGhhdCBpcyB0cnVlIGF0IGZpbGluZyBvcGVucyBvciBqb2lucyBhIGNsYWltIHdpbmRvdyoqCnJhdGhlciB0aGFuIHNldHRsaW5nLiBUaGUgZmlyc3Qgb25lIGZyZWV6ZXMgdGhlIGNlcnRpZmljYXRlOyB0aGUgcmVzdApxdWV1ZSB1cCBiZWhpbmQgaXQgYW5kIGFyZSBhbGwgcGFpZCB0b2dldGhlciwgcHJvIHJhdGEsIGF0IGNsb3NlLgozLiAqKkEgY2xhaW0gdGhhdCBpcyBmYWxzZSBhdCBmaWxpbmcsIHdpdGggbm8gd2luZG93IG9wZW4sIGlzIHJlamVjdGVkCm9uIHRoZSBzcG90KiogYW5kIGl0cyBib25kIGZvcmZlaXRlZC4gSXQgZGVsaWJlcmF0ZWx5IGRvZXMgbm90IG9wZW4gYQp3aW5kb3c6IGlmIGEgd3JvbmcgY2xhaW0gY291bGQgZnJlZXplIGEgY2VydGlmaWNhdGUgZm9yIDcyIGhvdXJzLAphbnlib2R5IGNvdWxkIGZyZWV6ZSBhbnkgY2VydGlmaWNhdGUgZm9yIHRoZSBwcmljZSBvZiB0aGUgbWluaW11bQpib25kLiBPbmNlIGEgd2luZG93ICppcyogb3BlbiBhIHdyb25nIGNsYWltIGlzIGFsbG93ZWQgdG8gam9pbiBpdCDigJQKaXQgY2hhbmdlcyBuAAAACWNoYWxsZW5nZQAAAAAAAAUAAAAAAAAACmNoYWxsZW5nZXIAAAAAABMAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAAAAAAKcHJvb2ZfdHlwZQAAAAAH0AAAAAlQcm9vZlR5cGUAAAAAAAAAAAAABnZpY3RpbQAAAAAAEwAAAAAAAAAFc3Rha2UAAAAAAAALAAAAAQAAAAY=",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAADwAAAAAAAAAAAAAACFJlZ2lzdHJ5AAAAAAAAAAAAAAAOQXVkaXRvclN0YWtpbmcAAAAAAAAAAAAAAAAADFJlc2VydmVWYXVsdAAAAAAAAAAAAAAACUZlZUVzY3JvdwAAAAAAAAAAAAAAAAAABVRva2VuAAAAAAAAAAAAAAAAAAAHQXJiaXRlcgAAAAAAAAAAeFRoZSBQYXltZW50Um91dGVyLCBzb3VyY2Ugb2YgdHJ1dGggZm9yIGBzcGVudGAgYW5kIGBwb3N0X2V4cGlyeV9zcGVudGAuClNldCBvbmNlLCBhZnRlciBpbml0aWFsaXplIOKAlCBzZWUgYHNldF9yb3V0ZXJgLgAAAAZSb3V0ZXIAAAAAAAAAAAByVGhlIFByZW1pdW1WYXVsdCwgc291cmNlIG9mIHRoZSBzdGVwLTQgcHJlbWl1bSBmb3JmZWl0dXJlLiBTZXQgb25jZSwKYWZ0ZXIgaW5pdGlhbGl6ZSDigJQgc2VlIGBzZXRfcHJlbWl1bV92YXVsdGAuAAAAAAAMUHJlbWl1bVZhdWx0AAAAAAAAADdXaGVyZSBzbGFzaGVkIHN0YWtlIGdvZXMsIGFuZCB0aGUgb25seSBwbGFjZSBpdCBtYXkgZ28uAAAAAAhUcmVhc3VyeQAAAAAAAAAAAAAACE1pblN0YWtlAAAAAQAAAAAAAAAJQ2hhbGxlbmdlAAAAAAAAAQAAAAYAAAAAAAAAAAAAAA5DaGFsbGVuZ2VDb3VudAAAAAAAAAAAAKVTdW0gb2YgY2hhbGxlbmdlciBib25kcyBjdXJyZW50bHkgaGVsZCBhbmQgc3RpbGwgb3dlZCBiYWNrLiBBbnl0aGluZyB0aGUKY29udHJhY3QgaG9sZHMgYWJvdmUgdGhpcyBpcyBmb3JmZWl0ZWQtYm9uZCBzdXJwbHVzLCB3aGljaCBpcyB3aGF0IGZ1bmRzCnRoZSBoeWdpZW5lIGJvdW50eS4AAAAAAAAJQm9uZHNIZWxkAAAAAAAAAQAAAGhERVNJR04tVjIgwqcxLiBUaGUgb3BlbiBjbGFpbSB3aW5kb3cgZm9yIGEgY2VydGlmaWNhdGUsIGlmIGFueS4gS2V5ZWQgYnkKYGNlcnRfaWRgLCBub3QgYnkgY2hhbGxlbmdlIGlkLgAAAAZXaW5kb3cAAAAAAAEAAAAGAAAAAQAAAMVTZXQgb25jZSBhIGNlcnRpZmljYXRlJ3Mgd2luZG93IGhhcyBjbG9zZWQgd2l0aCBhdCBsZWFzdCBvbmUgYWRtaXR0ZWQKY2xhaW0uIFRoZSBjZXJ0aWZpY2F0ZSBpcyBkZWFkLCBpdHMgcmVzZXJ2ZSBoYXMgYmVlbiBzcGVudCBhbmQgaXRzCmFsbG9jYXRpb24gcmV0aXJlZCwgc28gbm8gc2Vjb25kIHdpbmRvdyBtYXkgZXZlciBvcGVuIG9uIGl0LgAAAAAAAAdTZXR0bGVkAAAAAAEAAAAG",
        "AAAAAgAAAAAAAAAAAAAAB1ZlcmRpY3QAAAAABQAAAAAAAAAAAAAAB1BlbmRpbmcAAAAAAAAAAFJQcm92ZW4gYXQgZmlsaW5nLCBzdGlsbCB0cnVlIGF0IHdpbmRvdyBjbG9zZTogYWRtaXR0ZWQgdG8gdGhlIHByby1yYXRhCnNldHRsZW1lbnQuAAAAAAANQ2hhbGxlbmdlV2lucwAAAAAAAAAAAABWV3JvbmcgYXQgZmlsaW5nLiBUaGUgYm9uZCBpcyBmb3JmZWl0ZWQg4oCUIHRoaXMgaXMgdGhlIG9ubHkgb3V0Y29tZSB0aGF0CmZvcmZlaXRzIG9uZS4AAAAAAA5DaGFsbGVuZ2VGYWlscwAAAAAAAAAAAKRERVNJR04tVjIgwqcyLiBSaWdodCBhdCBmaWxpbmcsIHJlbWVkaWVkIGJ5IHRoZSBvcGVyYXRvciBiZWZvcmUgdGhlCndpbmRvdyBjbG9zZWQuIFRoZSBib25kIGlzIHJldHVybmVkICoqaW4gZnVsbCoqLCB0aGUgY2VydGlmaWNhdGUKc3Vydml2ZXMgYW5kIG5vYm9keSBpcyBzbGFzaGVkLgAAAAVDdXJlZAAAAAAAAAAAAAC6QW4gYXJiaXRlci1nYXRlZCBjbGFpbSAoYEZha2VTaWduYXR1cmVgKSB0aGUgYXJiaXRlciBuZXZlciBydWxlZCBvbgpiZWZvcmUgdGhlIHdpbmRvdyBjbG9zZWQuIFRoZSBib25kIGlzIHJldHVybmVkIGluIGZ1bGw6IGEgY2xhaW0gbm9ib2R5Cmp1ZGdlZCBpcyBub3QgYSBjbGFpbSB0aGUgY2hhbGxlbmdlciBnb3Qgd3JvbmcuAAAAAAANVW5hZGp1ZGljYXRlZAAAAA==",
        "AAAAAAAAAAAAAAAKZ2V0X3JvdXRlcgAAAAAAAAAAAAEAAAAT",
        "AAAAAAAAADVUaGUgY2xhaW0gd2luZG93LCBpZiBvbmUgaXMgb3BlbiBvbiB0aGlzIGNlcnRpZmljYXRlLgAAAAAAAApnZXRfd2luZG93AAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAD6AAAB9AAAAALQ2xhaW1XaW5kb3cA",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAACAAAAAAAAAAIcmVnaXN0cnkAAAATAAAAAAAAAA9hdWRpdG9yX3N0YWtpbmcAAAAAEwAAAAAAAAANcmVzZXJ2ZV92YXVsdAAAAAAAABMAAAAAAAAACmZlZV9lc2Nyb3cAAAAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAHYXJiaXRlcgAAAAATAAAAAAAAAAh0cmVhc3VyeQAAABMAAAAAAAAACW1pbl9zdGFrZQAAAAAAAAsAAAAA",
        "AAAAAAAAAHtXaGV0aGVyIHRoaXMgY2VydGlmaWNhdGUgaGFzIGFscmVhZHkgYmVlbiBzZXR0bGVkIHRocm91Z2ggYSBjbG9zZWQKd2luZG93LiBBIHNldHRsZWQgY2VydGlmaWNhdGUgYWNjZXB0cyBubyBmdXJ0aGVyIGNsYWltcy4AAAAACmlzX3NldHRsZWQAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAB",
        "AAAAAAAAA4dQb2ludCB0aGlzIGNvbnRyYWN0IGF0IHRoZSBQYXltZW50Um91dGVyLgoKU2VwYXJhdGUgZnJvbSBgaW5pdGlhbGl6ZWAgZm9yIG9uZSByZWFzb246IHRoZSBhcmd1bWVudCBsaXN0IG9mIGEKYHB1YiBmbmAgaW4gYSBgI1tjb250cmFjdGltcGxdYCBibG9jayBpcyB0aGUgY29udHJhY3QncyBBQkksIGFuZApgaW5pdGlhbGl6ZWAgYWxyZWFkeSBoYXMgZWlnaHQgcGFyYW1ldGVycyB0aGF0IHRoZSBkZXBsb3kgc2NyaXB0IGFuZCB0aGUKY29tbWl0dGVkIGJpbmRpbmdzIHBhc3MgcG9zaXRpb25hbGx5LiBBIG5pbnRoIHdvdWxkIGJyZWFrIGJvdGggYXQKcnVudGltZSBmb3Igbm8gZ2Fpbiwgc28gdGhlIHdpcmluZyBpcyBhIHNlY29uZCBvbmUtc2hvdCBjYWxsIGluc3RlYWQuCgoqKkFyYml0ZXItYXV0aG9yaXplZCwgYW5kIHNldHRhYmxlIGV4YWN0bHkgb25jZS4qKiBgaW5pdGlhbGl6ZWAgaXMKdW5hdXRoZW50aWNhdGVkIChhIGtub3duIGRlZmVjdCwgdGVzdGVkIGFzIHN1Y2gpLCBhbmQgY29weWluZyB0aGF0IGhlcmUKd291bGQgYmUgbWF0ZXJpYWxseSB3b3JzZTogd2hvZXZlciB3aW5zIHRoZSByYWNlIHRvIG5hbWUgdGhlIHJvdXRlciBuYW1lcwp0aGUgY29udHJhY3QgdGhhdCByZXBvcnRzIGBzcGVudGAsIGFuZCBhIGx5aW5nIHJvdXRlciBjYW4gaW52YWxpZGF0ZSBhbnkKY2VydGlmaWNhdGUgaXQgbGlrZXMuIFRoZSBhcmJpdGVyIGlzIGFscmVhZHkgYSBuYW1lZCB0cnVzdGVkIHBhcnR5IGF0CmBpbml0aWFsaXplYCwgc28gcmVxdWlyaW5nIHRoZWlyIHNpZ25hdHVyZSBncmFudHMgbm8gbmV3IHRydXN0IGFuZCBjbG9zZXMKdGhlIHJhY2UuIFRoZXJlIGlzIG5vIHJlLXBvaW50aW5nLCBmb3IgdGhlIHNhbWUgcmVhc29uIHRoZSB0cmVhc3VyeQpjYW5ub3QgYmUgcmUtcG9pbnRlZC4AAAAACnNldF9yb3V0ZXIAAAAAAAEAAAAAAAAABnJvdXRlcgAAAAAAEwAAAAA=",
        "AAAAAQAAAAAAAAAAAAAACUNoYWxsZW5nZQAAAAAAAAsAAAB/RmFsc2Ugb25seSBmb3IgYSBgRmFrZVNpZ25hdHVyZWAgY2xhaW0gdGhlIGFyYml0ZXIgaGFzIG5vdCBydWxlZCBvbiB5ZXQuCkV2ZXJ5IG9uLWNoYWluIHByZWRpY2F0ZSBhZGp1ZGljYXRlcyBpdHNlbGYgYXQgZmlsaW5nLgAAAAALYWRqdWRpY2F0ZWQAAAAAAQAAALNUcnVlIHdoZW4gYGhhcm1gIHdhcyAqKnN0YXRlZCBieSB0aGUgYXJiaXRlcioqIHJhdGhlciB0aGFuIGNvbXB1dGVkIGJ5IGEKcHJlZGljYXRlLiBUaGUgZGlzdGluY3Rpb24gZGVjaWRlcyBob3cgdGhlIG51bWJlciBhZ2dyZWdhdGVzIGFjcm9zcyBhCmNsYWltIHdpbmRvdyDigJQgc2VlIGBjbG9zZV93aW5kb3dgLgAAAAAKYXJiaXRyYXRlZAAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAApjaGFsbGVuZ2VyAAAAAAATAAAAh0xlZGdlciB0aW1lIHRoZSBjbGFpbSB3YXMgZmlsZWQuIERFU0lHTi1WMiDCpzI6IHRoZSBwcmVkaWNhdGUgaXMKZXZhbHVhdGVkIGhlcmUsIGFuZCB0aGlzIGlzIHRoZSBpbnN0YW50IHRoZSByZWNvcmRlZCBzdGF0ZSBiZWxvbmdzIHRvLgAAAAAIZmlsZWRfYXQAAAAGAAAAkFRoZSBoYXJtIHRoZSBwcmVkaWNhdGUgcXVhbnRpZmllZCAqKmF0IGZpbGluZyoqLCBpbiBzdHJvb3BzLiBUaGlzIGlzIHRoZQpudW1iZXIgdGhlIHByby1yYXRhIHNldHRsZW1lbnQgZGl2aWRlcyBieTsgbGl2ZSBzdGF0ZSBuZXZlciByZXNpemVzIGl0LgAAAARoYXJtAAAACwAAAAAAAAAKcHJvb2ZfdHlwZQAAAAAH0AAAAAlQcm9vZlR5cGUAAAAAAADKREVTSUdOLVYyIMKnMi4gVGhlIHByZWRpY2F0ZSdzIHZhbHVlICoqYXQgZmlsaW5nKiosIHJlY29yZGVkIG9uY2UgYW5kCm5ldmVyIHJlY29tcHV0ZWQuIFdoYXQgd2FzIHRydWUgd2hlbiB0aGUgY2hhbGxlbmdlIHdhcyBmaWxlZCBzdGF5cyB0cnVlLApzbyBhbiBvcGVyYXRvciBjYW5ub3QgZmxpcCBpdCB0byBmYWxzZSBhbmQgcG9ja2V0IHRoZSBib25kLgAAAAAABnByb3ZlbgAAAAAAAQAAAAAAAAAFc3Rha2UAAAAAAAALAAAAAAAAAAd2ZXJkaWN0AAAAB9AAAAAHVmVyZGljdAAAAAAAAAAABnZpY3RpbQAAAAAAEw==",
        "AAAAAgAAAAAAAAAAAAAACVByb29mVHlwZQAAAAAAAAQAAAAAAAAAAAAAABNJbnN1ZmZpY2llbnRSZXNlcnZlAAAAAAAAAAAAAAAADUJvdW5kRXhjZWVkZWQAAAAAAAAAAAAAAAAAABJFeHBpcmVkQ2VydGlmaWNhdGUAAAAAAAAAAAAAAAAADUZha2VTaWduYXR1cmUAAAA=",
        "AAAAAAAAAAAAAAAMY2xvc2Vfd2luZG93AAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAA==",
        "AAAAAAAAAAAAAAAMZ2V0X3RyZWFzdXJ5AAAAAAAAAAEAAAAT",
        "AAAAAQAAAtZBIHN0cnVjdHVyYWwgbWlycm9yIG9mIGBwYXltZW50X3JvdXRlcjo6UG9zdEV4cGlyeWAuCgpUaGUgQ2hhbGxlbmdlTWFuYWdlciByZWFkcyB0aGUgcm91dGVyIG92ZXIgYGludm9rZV9jb250cmFjdGAsIHNvIGl0IG5lZWRzIHRoZQpyZXR1cm4gdHlwZSBsb2NhbGx5LiBBIGAjW2NvbnRyYWN0dHlwZV1gIHN0cnVjdCBpcyBlbmNvZGVkIGFzIGEgbWFwIGtleWVkIGJ5CmZpZWxkICoqbmFtZSoqLCBzbyB0aGlzIGRlY29kZXMgdGhlIHJvdXRlcidzIHZhbHVlIGFzIGxvbmcgYXMgdGhlIG5hbWVzIGFuZAp0eXBlcyBtYXRjaC4gSXQgaXMgZHVwbGljYXRlZCByYXRoZXIgdGhhbiBpbXBvcnRlZCB0byBrZWVwIHRoZSByb3V0ZXIgb3V0IG9mCnRoaXMgY3JhdGUncyBkZXBlbmRlbmN5IGdyYXBoIOKAlCB0aGUgY29udHJhY3RzIGFyZSBkZXBsb3llZCBhbmQgdXBncmFkZWQKaW5kZXBlbmRlbnRseSwgYW5kIGEgY29tcGlsZS10aW1lIGRlcGVuZGVuY3kgd291bGQgbm90IG1ha2UgdGhlIG9uLWNoYWluIEFCSQphbnkgbW9yZSBjb3VwbGVkIHRoYW4gaXQgYWxyZWFkeSBpcy4gSWYgeW91IGNoYW5nZSBgUG9zdEV4cGlyeWAgaW4gdGhlCnJvdXRlciwgY2hhbmdlIGl0IGhlcmU7IGBleHBpcmVkX2NlcnRpZmljYXRlX3VwaG9sZHNfd2hlbl9hbGxfdGhyZWVfY29uZGl0aW9uc19ob2xkYAppbiB0aGUgaW50ZWdyYXRpb24gaGFybmVzcyBmYWlscyBsb3VkbHkgaWYgdGhlIHR3byBkcmlmdC4AAAAAAAAAAAAKUG9zdEV4cGlyeQAAAAAABQAAAAAAAAAFY291bnQAAAAAAAAEAAAAAAAAAAhmaXJzdF9hdAAAAAYAAAAAAAAAC21heF9wYXltZW50AAAAAAsAAAAAAAAADm1heF9wYXltZW50X2F0AAAAAAAGAAAAAAAAAAV0b3RhbAAAAAAAAAs=",
        "AAAAAAAAAAAAAAANZ2V0X2NoYWxsZW5nZQAAAAAAAAEAAAAAAAAADGNoYWxsZW5nZV9pZAAAAAYAAAABAAAH0AAAAAlDaGFsbGVuZ2UAAAA=",
        "AAAAAQAAARZERVNJR04tVjIgwqcxLiBUaGUgY2xhaW0gd2luZG93IG9wZW5lZCBieSB0aGUgZmlyc3QgdmFsaWQgY2hhbGxlbmdlIGFnYWluc3QgYQpjZXJ0aWZpY2F0ZS4KCkEgd2luZG93IGlzIGEgKmNlcnRpZmljYXRlLWxldmVsKiBvYmplY3QsIG5vdCBhIGNoYWxsZW5nZS1sZXZlbCBvbmUuIFRoYXQgaXMKdGhlIHdob2xlIHBvaW50OiBzZXR0bGVtZW50IHJ1bnMgb25jZSwgb3ZlciBldmVyeSBjbGFpbSB0aGUgd2luZG93IGFkbWl0dGVkLApzbyBiZWluZyBmaXJzdCBpcyB3b3J0aCBub3RoaW5nLgAAAAAAAAAAAAtDbGFpbVdpbmRvdwAAAAAEAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAACpRXZlcnkgY2xhaW0gZmlsZWQgYWdhaW5zdCB0aGlzIGNlcnRpZmljYXRlIHdoaWxlIHRoZSB3aW5kb3cgd2FzIG9wZW4sCmluIGZpbGluZyBvcmRlci4gVGhlIG9yZGVyIGlzIHJlY29yZGVkIGZvciBhdWRpdGFiaWxpdHkgb25seSDigJQgbm8gcGF5b3V0CmFueXdoZXJlIGJlbG93IHJlYWRzIGl0LgAAAAAAAAZjbGFpbXMAAAAAA+oAAAAGAAAAAAAAAAljbG9zZXNfYXQAAAAAAAAGAAAAAAAAAAlvcGVuZWRfYXQAAAAAAAAG",
        "AAAAAAAAAAAAAAAOZ2V0X2JvbmRzX2hlbGQAAAAAAAAAAAABAAAACw==",
        "AAAAAAAAAHhGb3JmZWl0ZWQgYm9uZHMgdGhlIGNvbnRyYWN0IGhvbGRzIGJleW9uZCB3aGF0IGl0IHN0aWxsIG93ZXMgYmFjay4gVGhpcwppcyB0aGUgb25seSBwb3QgdGhlIGh5Z2llbmUgYm91bnR5IGlzIHBhaWQgZnJvbS4AAAAPZ2V0X2JvdW50eV9wb29sAAAAAAAAAAABAAAACw==",
        "AAAAAAAAAF9MZWRnZXIgdGltZSBhdCB3aGljaCB0aGlzIGNlcnRpZmljYXRlJ3Mgb3BlbiB3aW5kb3cgbWF5IGJlIGNsb3NlZC4KYDBgIG1lYW5zIG5vIHdpbmRvdyBpcyBvcGVuLgAAAAAQd2luZG93X2Nsb3Nlc19hdAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAG",
        "AAAAAAAAAAAAAAARZ2V0X3ByZW1pdW1fdmF1bHQAAAAAAAAAAAAAAQAAABM=",
        "AAAAAAAAAedUaGUgdmF1bHQgZXZlcnkgcmVzZXJ2ZSBwcm9vZiBhbmQgZXZlcnkgc2V0dGxlbWVudCBwYXltZW50IHJlYWRzLgoKRXhwb3NlZCBmb3IgdGhlIFJlZ2lzdHJ5LCB3aGljaCBuZWVkcyBpdCBhdCBgYXR0ZXN0YCAoREVTSUdOLVYyIMKnNCkgdG8KY2hlY2sgYSBjZXJ0aWZpY2F0ZSdzIHJlc2VydmUgaXMgZnVuZGVkICphZ2FpbnN0IHRoZSBzYW1lIGNvbnRyYWN0IHRoaXMKb25lIHdpbGwgbGF0ZXIgbWVhc3VyZSB0aGUgc2hvcnRmYWxsIG9uKi4gUHVibGlzaGluZyB0aGUgYWRkcmVzcyByYXRoZXIKdGhhbiBsZXR0aW5nIHRoZSBSZWdpc3RyeSBob2xkIGl0cyBvd24gY29weSBpcyB3aGF0IG1ha2VzIHRoZSB0d28KY2hlY2tzIGFncmVlIGJ5IGNvbnN0cnVjdGlvbiBpbnN0ZWFkIG9mIGJ5IGNvbnZlbnRpb246IHRoZXJlIGlzIGV4YWN0bHkKb25lIGBEYXRhS2V5OjpSZXNlcnZlVmF1bHRgIGluIHRoZSBkZXBsb3ltZW50LCBhbmQgaXQgaXMgdGhpcyBvbmUuAAAAABFnZXRfcmVzZXJ2ZV92YXVsdAAAAAAAAAAAAAABAAAAEw==",
        "AAAAAAAAAMFXaGV0aGVyIHN0ZXAgNCBpcyBsaXZlLiBGYWxzZSBtZWFucyBhIGRlcGxveW1lbnQgdGhhdCBuZXZlciBjYWxsZWQKYHNldF9wcmVtaXVtX3ZhdWx0YCwgaW4gd2hpY2ggY2FzZSBzZXR0bGVtZW50IHNpbGVudGx5IHNraXBzIHRoZSBwcmVtaXVtCnN0ZXAg4oCUIHNlZSB0aGUgY29tbWVudCBvbiBzdGVwIDQgaW4gYHNldHRsZV9mcmF1ZGAuAAAAAAAAEWhhc19wcmVtaXVtX3ZhdWx0AAAAAAAAAAAAAAEAAAAB",
        "AAAAAAAAAw9Qb2ludCB0aGlzIGNvbnRyYWN0IGF0IHRoZSBQcmVtaXVtVmF1bHQsIHdoaWNoIG93bnMgc3RlcCA0IG9mIHRoZQpzZXR0bGVtZW50IHdhdGVyZmFsbC4KCkEgc2Vjb25kIG9uZS1zaG90IGNhbGwgcmF0aGVyIHRoYW4gYSB0ZW50aCBgaW5pdGlhbGl6ZWAgYXJndW1lbnQsIGZvcgpleGFjdGx5IHRoZSByZWFzb25zIHNwZWxsZWQgb3V0IG9uIGBzZXRfcm91dGVyYDogYGluaXRpYWxpemVgJ3MgYXJndW1lbnQKbGlzdCBpcyB0aGUgb24tY2hhaW4gQUJJLCB0aGUgZGVwbG95IHNjcmlwdCBhbmQgdGhlIGNvbW1pdHRlZCBiaW5kaW5ncwpwYXNzIGl0IHBvc2l0aW9uYWxseSwgYW5kIHdpZGVuaW5nIGl0IHdvdWxkIGJyZWFrIGJvdGggYXQgcnVudGltZS4KCioqQXJiaXRlci1hdXRob3JpemVkLCBhbmQgc2V0dGFibGUgZXhhY3RseSBvbmNlLioqIFRoZSB2YXVsdCBuYW1lZCBoZXJlCmlzIGhhbmRlZCB0aGUgY2VydGlmaWNhdGUncyBmb3JmZWl0ZWQgcHJlbWl1bSBhbmQgdG9sZCB3aGVyZSB0byBzZW5kIGl0LApzbyB3aG9ldmVyIG5hbWVzIGl0IGNhbiBuYW1lIGEgY29udHJhY3QgdGhhdCBrZWVwcyB0aGUgbW9uZXkuIFRoZSBhcmJpdGVyCmlzIGFscmVhZHkgYSB0cnVzdGVkIHBhcnR5IGF0IGBpbml0aWFsaXplYCwgc28gcmVxdWlyaW5nIHRoZWlyIHNpZ25hdHVyZQpncmFudHMgbm8gbmV3IHRydXN0IGFuZCBjbG9zZXMgdGhlIHJhY2UuIFRoZXJlIGlzIG5vIHJlLXBvaW50aW5nLCBmb3IgdGhlCnNhbWUgcmVhc29uIHRoZSB0cmVhc3VyeSBjYW5ub3QgYmUgcmUtcG9pbnRlZC4AAAAAEXNldF9wcmVtaXVtX3ZhdWx0AAAAAAAAAQAAAAAAAAANcHJlbWl1bV92YXVsdAAAAAAAABMAAAAA",
        "AAAAAAAABABERVNJR04tVjIgwqcxMC4gQ2xvc2UgYSB3aW5kb3cgZWFybHksIG9uIHRoZSBhcmJpdGVyJ3MgcmVqZWN0aW9uIG9mIHRoZQpvbmx5IHRoaW5nIGtlZXBpbmcgaXQgb3Blbi4KCiMjIFRoZSBhYnVzZSB0aGlzIGNsb3NlcwoKYEZha2VTaWduYXR1cmVgIGhhcyAqKm5vIHByZWRpY2F0ZSoqLiBBIGNsYWltIGNhcnJ5aW5nIGl0IGNhbm5vdCBiZQpldmFsdWF0ZWQgYXQgZmlsaW5nIHRoZSB3YXkgYSBmYWxzZSBgSW5zdWZmaWNpZW50UmVzZXJ2ZWAgY2xhaW0gaXMsIHNvCml0IGNhbm5vdCBiZSByZWplY3RlZCBvbiB0aGUgc3BvdCDigJQgaXQgb3BlbnMgYSB3aW5kb3cgYW5kIHdhaXRzIGZvciB0aGUKYXJiaXRlci4gSXQgdXNlZCB0byBmcmVlemUgdGhlIGNlcnRpZmljYXRlIGZvciB0aG9zZSA3MiBob3VycyBhcyB3ZWxsOgpubyBhdHRlc3RhdGlvbiwgbm8gcmVzZXJ2ZSB3aXRoZHJhd2FsLCBubyBhbGxvY2F0aW9uIHJlbGVhc2UuIEFueW9uZQpjb3VsZCBidXkgdGhhdCBmcmVlemUgYWdhaW5zdCBhbnkgY2VydGlmaWNhdGUgZm9yIHRoZSBwcmljZSBvZiB0aGUKbWluaW11bSBib25kLCBhbmQgYW4gYXJiaXRlciBydWxpbmcgdGhlIGNsYWltIGZhbHNlIGRpZCBub3RoaW5nIHRvCnNob3J0ZW4gaXQuCgoqKlIzIGhhcyBzaW5jZSByZW1vdmVkIHRoZSBmcmVlemUgZnJvbSBhcmJpdGVyLWdhdGVkIGNsYWltcyBlbnRpcmVseSoqLApzbyB0aGUgYWJ1c2UgdGhpcyBjYWxsIHdhcyBidWlsdCBmb3Igbm8gbG9uZ2VyIGhhcyBhbnl0aGluZyB0byBidXkuIFRoaXMKY2FsbCBpcyBub3QgdGhlcmVieSByZWR1bmRhbnQ6IGFuIG9wZW4gd2luZG93IHN0aWxsIGJsb2NrcyBhIHNlY29uZAp3aW5kb3cgb24gdGhlIHNhbWUgY2VydGlmaWNhdGUgYW5kIHN0aWxsIGhvbGRzIHRoZSBjbGFpbWFudHMnIGJvbmRzLCBhbmQKZW5kaW5nIGl0IG9uIHRoZSBhcmJpdGVyJ3MgcmVqZWN0aW9uIGlzIHRoZSByaWdodCBvdXRjb21lIHJhdGhlciB0aGFuCm1lcmVseSB0aGUgY2hlYXAgb25lLgoKVGhlIGZpeCBpcyB0byBtYWtlAAAAEmNsb3NlX3dpbmRvd19lYXJseQAAAAAAAQAAAAAAAAAMY2hhbGxlbmdlX2lkAAAABgAAAAA=",
        "AAAAAAAABABBcmJpdGVyIGFkanVkaWNhdGlvbjogYSB2ZXJkaWN0IGFuZCBhICoqcXVhbnRpdHkqKiBvbiBvbmUgbGl2ZSBjbGFpbS4KVGhpcyBpcyBhbiBleHBsaWNpdCB0cnVzdCBhc3N1bXB0aW9uOyB0aGUgYXJiaXRlciBpcyBuYW1lZCBhdApgaW5pdGlhbGl6ZWAuCgpJdCByZWFjaGVzIGFueSBjbGFpbSBpbnNpZGUgYW4gb3BlbiB3aW5kb3csIG5vdCBqdXN0IGBGYWtlU2lnbmF0dXJlYC4KVGhhdCBpcyB3aGF0IGxldHMgYEJvdW5kRXhjZWVkZWRgIGFuZCBgRXhwaXJlZENlcnRpZmljYXRlYCDigJQgd2hvc2UKb24tY2hhaW4gcHJlZGljYXRlcyBhcmUgdHJ1ZSBidXQgd2hvc2UgY291bnRlcnMgYXJlIG5ldmVyIGEgbG9zcyDigJQgYmUKZ2l2ZW4gYW4gYXNzZXNzZWQgaGFybSBhbmQgc2V0dGxlIHRocm91Z2ggdGhlIGZ1bGwgd2F0ZXJmYWxsIGluc3RlYWQgb2YKaHlnaWVuZSBtb2RlLgoKV0hBVCBJVCBDQU5OT1QgUkVBQ0gg4oCUICoqUjQqKiwgYW5kIHRoaXMgaXMgREVTSUdOLVYyIMKnMiB3b3JraW5nIGluIGJvdGgKZGlyZWN0aW9ucy4gT24gYSBjbGFpbSBjYXJyeWluZyBhbiBvbi1jaGFpbiBwcmVkaWNhdGUgdGhlIGFyYml0ZXIgbWF5CkFERCB0byB3aGF0IHRoZSBjb250cmFjdCBwcm92ZWQgYW5kIG1heSBuZXZlciBDT05UUkFESUNUIGl0OiB0aGUgdmVyZGljdAptdXN0IG1hdGNoIHdoYXQgdGhlIHByZWRpY2F0ZSByZWNvcmRlZCBhdCBmaWxpbmcsIGFuZCB0aGUgaGFybSBtYXkgbm90CmZhbGwgYmVsb3cgdGhlIG51bWJlciBpdCBjb21wdXRlZC4gVGhlIHJvdXRlciBhbmQgdGhlIHZhdWx0IGFyZSB0aGUKc291cmNlIG9mIHRydXRoIGZvciB3aGF0IHRoZXkgbWVhc3VyZSDigJQgbm8gaHVtYW4gbWF5IGRlY2xhcmUgYSBicmVhY2gKdGhleSBzYXkgZGlkIG5vdCBoYXBwZW4sIGFuZCBub25lIG1heSBkZW55IG9uZSB0aGF0IGRpZC4gVGhlIHJ1bGUgaXMKZW5mb3JjZWQgaW4gdGhlIGJvZHksIHdoZXJlIHRoZSByZWFzb25pbmcgaXMgd3JpdHRlbiBvdXQgaW4gZnVsbC4KCk9ubHkgYEZha2VTaWduAAAAEnJlc29sdmVfYnlfYXJiaXRlcgAAAAAAAwAAAAAAAAAMY2hhbGxlbmdlX2lkAAAABgAAAAAAAAAMZnJhdWRfcHJvdmVuAAAAAQAAAAAAAAAEaGFybQAAAAsAAAAA",
        "AAAAAAAAAAAAAAATZ2V0X2NoYWxsZW5nZV9jb3VudAAAAAAAAAAAAQAAAAY=",
        "AAAAAAAAADZIb3cgbG9uZyBhIGNsYWltIHdpbmRvdyBzdGF5cyBvcGVuLCBpbiBsZWRnZXIgc2Vjb25kcy4AAAAAABhnZXRfY2xhaW1fd2luZG93X3NlY29uZHMAAAAAAAAAAQAAAAY=" ]),
      options
    )
  }
  public readonly fromJSON = {
    challenge: this.txFromJSON<u64>,
        get_router: this.txFromJSON<string>,
        get_window: this.txFromJSON<Option<ClaimWindow>>,
        initialize: this.txFromJSON<null>,
        is_settled: this.txFromJSON<boolean>,
        set_router: this.txFromJSON<null>,
        close_window: this.txFromJSON<null>,
        get_treasury: this.txFromJSON<string>,
        get_challenge: this.txFromJSON<Challenge>,
        get_bonds_held: this.txFromJSON<i128>,
        get_bounty_pool: this.txFromJSON<i128>,
        window_closes_at: this.txFromJSON<u64>,
        get_premium_vault: this.txFromJSON<string>,
        get_reserve_vault: this.txFromJSON<string>,
        has_premium_vault: this.txFromJSON<boolean>,
        set_premium_vault: this.txFromJSON<null>,
        close_window_early: this.txFromJSON<null>,
        resolve_by_arbiter: this.txFromJSON<null>,
        get_challenge_count: this.txFromJSON<u64>,
        get_claim_window_seconds: this.txFromJSON<u64>
  }
}