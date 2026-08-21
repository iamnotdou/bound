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
    contractId: "CCJTY2VYHQZ7OQE6NX7QFL7JL766YMTNKGHZXGAPTPKV5IUS2AK5UREF",
  }
} as const

export type DataKey = {tag: "ChallengeManager", values: void} | {tag: "AuditorStaking", values: void} | {tag: "CertCount", values: void} | {tag: "Certificate", values: readonly [u64]} | {tag: "AgentCert", values: readonly [string]} | {tag: "ClaimFreeze", values: readonly [u64]};

export type CertStatus = {tag: "Pending", values: void} | {tag: "Verified", values: void} | {tag: "Invalid", values: void};


export interface Certificate {
  agent: string;
  auditor: Option<string>;
  auditor_stake_snapshot: i128;
  auditor_staking_contract: string;
  bound: i128;
  expires_at: u64;
  issued_at: u64;
  operator: string;
  reserve_amount: i128;
  reserve_vault_contract: string;
  status: CertStatus;
}


export interface VerifyResult {
  auditor: Option<string>;
  auditor_stake: i128;
  bound: i128;
  expires_at: u64;
  reserve: i128;
  status: CertStatus;
  valid: boolean;
}

export interface Client {
  /**
   * Construct and simulate a attest transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Sadece registered auditor → VERIFIED.
   * 
   * `allocation` is how much of the auditor's **free** stake stands behind
   * this one certificate. It is a parameter rather than a derived number
   * because the alternatives are both worse: allocating the auditor's whole
   * stake reproduces v1, where one bad certificate destroys an entire book;
   * allocating a protocol-fixed amount would price every certificate the
   * same regardless of the bound it backs. The auditor is the party pricing
   * the risk, so the auditor names the number — and AuditorStaking enforces
   * the two limits that matter: it must be at least `min_stake` (the same
   * floor `is_registered` uses, now applied per certificate rather than per
   * auditor) and it cannot exceed the auditor's free stake.
   * 
   * The allocation is locked until this certificate's settlement deadline,
   * not its expiry — see `CHALLENGE_WINDOW_SECONDS`.
   */
  attest: ({auditor, cert_id, allocation}: {auditor: string, cert_id: u64, allocation: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a verify transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  verify: ({agent}: {agent: string}, options?: MethodOptions) => Promise<AssembledTransaction<VerifyResult>>

  /**
   * Construct and simulate a publish transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  publish: ({operator, agent, bound, reserve_amount, expires_at, reserve_vault_contract, auditor_staking_contract}: {operator: string, agent: string, bound: i128, reserve_amount: i128, expires_at: u64, reserve_vault_contract: string, auditor_staking_contract: string}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a is_frozen transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Whether a claim window is open on this certificate right now.
   */
  is_frozen: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({challenge_manager, auditor_staking}: {challenge_manager: string, auditor_staking: string}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a invalidate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  invalidate: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_cert_id transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_id: ({agent}: {agent: string}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_cert_agent transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The agent named on the certificate.
   * 
   * The ChallengeManager needs it to answer "has this certificate been
   * superseded?": `get_cert_id(agent)` is the agent's *current* certificate,
   * and if that is no longer `cert_id` the operator has published a fresh one
   * for the same agent. That is what renewal means in this registry — there
   * is no in-place extension, because `expires_at` is immutable once
   * published.
   */
  get_cert_agent: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_cert_bound transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The bound the certificate advertises. The ChallengeManager reads this
   * for two things: the `BoundExceeded` comparison against the router's
   * spend counter, and the de-minimis floor of the `ExpiredCertificate`
   * predicate, which is a percentage of this number rather than a flat
   * amount.
   */
  get_cert_bound: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_cert_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_count: (options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_certificate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_certificate: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Certificate>>

  /**
   * Construct and simulate a get_cert_auditor transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_auditor: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_cert_reserve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_reserve: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_claim_freeze transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Ledger time until which an open claim window holds this certificate
   * frozen. `0` means no window is open.
   */
  get_claim_freeze: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a is_cert_verified transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Whether the certificate is currently attested and not invalidated.
   * Deliberately says nothing about expiry: a predicate about post-expiry
   * activity has to be able to ask this question after `expires_at`.
   */
  is_cert_verified: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a set_claim_freeze transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * DESIGN-V2 §1. Freeze this certificate until `until`, or lift the freeze
   * by passing `0`.
   * 
   * ChallengeManager-only, exactly like `invalidate`: the claim window is
   * the ChallengeManager's concept and nobody else may open or close one.
   * The freeze is expressed as a settlement deadline rather than a separate
   * flag so that both money contracts get it for free — they already refuse
   * to release before `get_cert_settlement_deadline`.
   */
  set_claim_freeze: ({cert_id, until}: {cert_id: u64, until: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_cert_operator transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_operator: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_cert_issued_at transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * When the certificate was published.
   * 
   * The PremiumVault reads this together with `get_cert_expires_at` to price
   * coverage over `expires_at - issued_at`. Both fields are immutable once
   * published, which is the point: a premium priced from `now` would be a
   * function of when the operator chose to pay it, and an operator would
   * simply wait until the instant before expiry and buy a year of coverage
   * for a day's price.
   */
  get_cert_issued_at: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_cert_expires_at transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_expires_at: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_cert_settlement_deadline transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The instant after which nothing can still be proven against this
   * certificate, and therefore the instant its collateral may unwind.
   * 
   * Both the ReserveVault (operator's reserve) and AuditorStaking (auditor's
   * allocation) lock to this, so the two never drift apart.
   */
  get_cert_settlement_deadline: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

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
      new ContractSpec([ "AAAAAAAAA1xTYWRlY2UgcmVnaXN0ZXJlZCBhdWRpdG9yIOKGkiBWRVJJRklFRC4KCmBhbGxvY2F0aW9uYCBpcyBob3cgbXVjaCBvZiB0aGUgYXVkaXRvcidzICoqZnJlZSoqIHN0YWtlIHN0YW5kcyBiZWhpbmQKdGhpcyBvbmUgY2VydGlmaWNhdGUuIEl0IGlzIGEgcGFyYW1ldGVyIHJhdGhlciB0aGFuIGEgZGVyaXZlZCBudW1iZXIKYmVjYXVzZSB0aGUgYWx0ZXJuYXRpdmVzIGFyZSBib3RoIHdvcnNlOiBhbGxvY2F0aW5nIHRoZSBhdWRpdG9yJ3Mgd2hvbGUKc3Rha2UgcmVwcm9kdWNlcyB2MSwgd2hlcmUgb25lIGJhZCBjZXJ0aWZpY2F0ZSBkZXN0cm95cyBhbiBlbnRpcmUgYm9vazsKYWxsb2NhdGluZyBhIHByb3RvY29sLWZpeGVkIGFtb3VudCB3b3VsZCBwcmljZSBldmVyeSBjZXJ0aWZpY2F0ZSB0aGUKc2FtZSByZWdhcmRsZXNzIG9mIHRoZSBib3VuZCBpdCBiYWNrcy4gVGhlIGF1ZGl0b3IgaXMgdGhlIHBhcnR5IHByaWNpbmcKdGhlIHJpc2ssIHNvIHRoZSBhdWRpdG9yIG5hbWVzIHRoZSBudW1iZXIg4oCUIGFuZCBBdWRpdG9yU3Rha2luZyBlbmZvcmNlcwp0aGUgdHdvIGxpbWl0cyB0aGF0IG1hdHRlcjogaXQgbXVzdCBiZSBhdCBsZWFzdCBgbWluX3N0YWtlYCAodGhlIHNhbWUKZmxvb3IgYGlzX3JlZ2lzdGVyZWRgIHVzZXMsIG5vdyBhcHBsaWVkIHBlciBjZXJ0aWZpY2F0ZSByYXRoZXIgdGhhbiBwZXIKYXVkaXRvcikgYW5kIGl0IGNhbm5vdCBleGNlZWQgdGhlIGF1ZGl0b3IncyBmcmVlIHN0YWtlLgoKVGhlIGFsbG9jYXRpb24gaXMgbG9ja2VkIHVudGlsIHRoaXMgY2VydGlmaWNhdGUncyBzZXR0bGVtZW50IGRlYWRsaW5lLApub3QgaXRzIGV4cGlyeSDigJQgc2VlIGBDSEFMTEVOR0VfV0lORE9XX1NFQ09ORFNgLgAAAAZhdHRlc3QAAAAAAAMAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAphbGxvY2F0aW9uAAAAAAALAAAAAA==",
        "AAAAAAAAAAAAAAAGdmVyaWZ5AAAAAAABAAAAAAAAAAVhZ2VudAAAAAAAABMAAAABAAAH0AAAAAxWZXJpZnlSZXN1bHQ=",
        "AAAAAAAAAAAAAAAHcHVibGlzaAAAAAAHAAAAAAAAAAhvcGVyYXRvcgAAABMAAAAAAAAABWFnZW50AAAAAAAAEwAAAAAAAAAFYm91bmQAAAAAAAALAAAAAAAAAA5yZXNlcnZlX2Ftb3VudAAAAAAACwAAAAAAAAAKZXhwaXJlc19hdAAAAAAABgAAAAAAAAAWcmVzZXJ2ZV92YXVsdF9jb250cmFjdAAAAAAAEwAAAAAAAAAYYXVkaXRvcl9zdGFraW5nX2NvbnRyYWN0AAAAEwAAAAEAAAAG",
        "AAAAAAAAAD1XaGV0aGVyIGEgY2xhaW0gd2luZG93IGlzIG9wZW4gb24gdGhpcyBjZXJ0aWZpY2F0ZSByaWdodCBub3cuAAAAAAAACWlzX2Zyb3plbgAAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAB",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAABgAAAAAAAAAAAAAAEENoYWxsZW5nZU1hbmFnZXIAAAAAAAAAAAAAAA5BdWRpdG9yU3Rha2luZwAAAAAAAAAAAAAAAAAJQ2VydENvdW50AAAAAAAAAQAAAAAAAAALQ2VydGlmaWNhdGUAAAAAAQAAAAYAAAABAAAAAAAAAAlBZ2VudENlcnQAAAAAAAABAAAAEwAAAAEAAAFUREVTSUdOLVYyIMKnMS4gTGVkZ2VyIHRpbWUgdW50aWwgd2hpY2ggYW4gb3BlbiAqKmNsYWltIHdpbmRvdyoqIGZyZWV6ZXMKdGhpcyBjZXJ0aWZpY2F0ZS4gV3JpdHRlbiBvbmx5IGJ5IHRoZSBDaGFsbGVuZ2VNYW5hZ2VyLCB0aHJvdWdoCmBzZXRfY2xhaW1fZnJlZXplYCwgYW5kIGZvbGRlZCBpbnRvIGBnZXRfY2VydF9zZXR0bGVtZW50X2RlYWRsaW5lYCBzbwp0aGF0IHRoZSBmcmVlemUgcmV1c2VzIHRoZSBvbmUgbG9ja2luZyBtZWNoYW5pc20gdGhlIFJlc2VydmVWYXVsdCBhbmQKQXVkaXRvclN0YWtpbmcgYWxyZWFkeSByZWFkLCByYXRoZXIgdGhhbiBpbnZlbnRpbmcgYSBzZWNvbmQgb25lLgAAAAtDbGFpbUZyZWV6ZQAAAAABAAAABg==",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAAAgAAAAAAAAARY2hhbGxlbmdlX21hbmFnZXIAAAAAAAATAAAAAAAAAA9hdWRpdG9yX3N0YWtpbmcAAAAAEwAAAAA=",
        "AAAAAAAAAAAAAAAKaW52YWxpZGF0ZQAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAA==",
        "AAAAAAAAAAAAAAALZ2V0X2NlcnRfaWQAAAAAAQAAAAAAAAAFYWdlbnQAAAAAAAATAAAAAQAAAAY=",
        "AAAAAgAAAAAAAAAAAAAACkNlcnRTdGF0dXMAAAAAAAMAAAAAAAAAAAAAAAdQZW5kaW5nAAAAAAAAAAAAAAAACFZlcmlmaWVkAAAAAAAAAAAAAAAHSW52YWxpZAA=",
        "AAAAAQAAAAAAAAAAAAAAC0NlcnRpZmljYXRlAAAAAAsAAAAAAAAABWFnZW50AAAAAAAAEwAAAAAAAAAHYXVkaXRvcgAAAAPoAAAAEwAAAAAAAAAWYXVkaXRvcl9zdGFrZV9zbmFwc2hvdAAAAAAACwAAAAAAAAAYYXVkaXRvcl9zdGFraW5nX2NvbnRyYWN0AAAAEwAAAAAAAAAFYm91bmQAAAAAAAALAAAAAAAAAApleHBpcmVzX2F0AAAAAAAGAAAAAAAAAAlpc3N1ZWRfYXQAAAAAAAAGAAAAAAAAAAhvcGVyYXRvcgAAABMAAAAAAAAADnJlc2VydmVfYW1vdW50AAAAAAALAAAAAAAAABZyZXNlcnZlX3ZhdWx0X2NvbnRyYWN0AAAAAAATAAAAAAAAAAZzdGF0dXMAAAAAB9AAAAAKQ2VydFN0YXR1cwAA",
        "AAAAAAAAAZBUaGUgYWdlbnQgbmFtZWQgb24gdGhlIGNlcnRpZmljYXRlLgoKVGhlIENoYWxsZW5nZU1hbmFnZXIgbmVlZHMgaXQgdG8gYW5zd2VyICJoYXMgdGhpcyBjZXJ0aWZpY2F0ZSBiZWVuCnN1cGVyc2VkZWQ/IjogYGdldF9jZXJ0X2lkKGFnZW50KWAgaXMgdGhlIGFnZW50J3MgKmN1cnJlbnQqIGNlcnRpZmljYXRlLAphbmQgaWYgdGhhdCBpcyBubyBsb25nZXIgYGNlcnRfaWRgIHRoZSBvcGVyYXRvciBoYXMgcHVibGlzaGVkIGEgZnJlc2ggb25lCmZvciB0aGUgc2FtZSBhZ2VudC4gVGhhdCBpcyB3aGF0IHJlbmV3YWwgbWVhbnMgaW4gdGhpcyByZWdpc3RyeSDigJQgdGhlcmUKaXMgbm8gaW4tcGxhY2UgZXh0ZW5zaW9uLCBiZWNhdXNlIGBleHBpcmVzX2F0YCBpcyBpbW11dGFibGUgb25jZQpwdWJsaXNoZWQuAAAADmdldF9jZXJ0X2FnZW50AAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAAEw==",
        "AAAAAAAAARhUaGUgYm91bmQgdGhlIGNlcnRpZmljYXRlIGFkdmVydGlzZXMuIFRoZSBDaGFsbGVuZ2VNYW5hZ2VyIHJlYWRzIHRoaXMKZm9yIHR3byB0aGluZ3M6IHRoZSBgQm91bmRFeGNlZWRlZGAgY29tcGFyaXNvbiBhZ2FpbnN0IHRoZSByb3V0ZXIncwpzcGVuZCBjb3VudGVyLCBhbmQgdGhlIGRlLW1pbmltaXMgZmxvb3Igb2YgdGhlIGBFeHBpcmVkQ2VydGlmaWNhdGVgCnByZWRpY2F0ZSwgd2hpY2ggaXMgYSBwZXJjZW50YWdlIG9mIHRoaXMgbnVtYmVyIHJhdGhlciB0aGFuIGEgZmxhdAphbW91bnQuAAAADmdldF9jZXJ0X2JvdW5kAAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAACw==",
        "AAAAAAAAAAAAAAAOZ2V0X2NlcnRfY291bnQAAAAAAAAAAAABAAAABg==",
        "AAAAAQAAAAAAAAAAAAAADFZlcmlmeVJlc3VsdAAAAAcAAAAAAAAAB2F1ZGl0b3IAAAAD6AAAABMAAAAAAAAADWF1ZGl0b3Jfc3Rha2UAAAAAAAALAAAAAAAAAAVib3VuZAAAAAAAAAsAAAAAAAAACmV4cGlyZXNfYXQAAAAAAAYAAAAAAAAAB3Jlc2VydmUAAAAACwAAAAAAAAAGc3RhdHVzAAAAAAfQAAAACkNlcnRTdGF0dXMAAAAAAAAAAAAFdmFsaWQAAAAAAAAB",
        "AAAAAAAAAAAAAAAPZ2V0X2NlcnRpZmljYXRlAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAfQAAAAC0NlcnRpZmljYXRlAA==",
        "AAAAAAAAAAAAAAAQZ2V0X2NlcnRfYXVkaXRvcgAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAT",
        "AAAAAAAAAAAAAAAQZ2V0X2NlcnRfcmVzZXJ2ZQAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAL",
        "AAAAAAAAAGhMZWRnZXIgdGltZSB1bnRpbCB3aGljaCBhbiBvcGVuIGNsYWltIHdpbmRvdyBob2xkcyB0aGlzIGNlcnRpZmljYXRlCmZyb3plbi4gYDBgIG1lYW5zIG5vIHdpbmRvdyBpcyBvcGVuLgAAABBnZXRfY2xhaW1fZnJlZXplAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAY=",
        "AAAAAAAAAMlXaGV0aGVyIHRoZSBjZXJ0aWZpY2F0ZSBpcyBjdXJyZW50bHkgYXR0ZXN0ZWQgYW5kIG5vdCBpbnZhbGlkYXRlZC4KRGVsaWJlcmF0ZWx5IHNheXMgbm90aGluZyBhYm91dCBleHBpcnk6IGEgcHJlZGljYXRlIGFib3V0IHBvc3QtZXhwaXJ5CmFjdGl2aXR5IGhhcyB0byBiZSBhYmxlIHRvIGFzayB0aGlzIHF1ZXN0aW9uIGFmdGVyIGBleHBpcmVzX2F0YC4AAAAAAAAQaXNfY2VydF92ZXJpZmllZAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAB",
        "AAAAAAAAAalERVNJR04tVjIgwqcxLiBGcmVlemUgdGhpcyBjZXJ0aWZpY2F0ZSB1bnRpbCBgdW50aWxgLCBvciBsaWZ0IHRoZSBmcmVlemUKYnkgcGFzc2luZyBgMGAuCgpDaGFsbGVuZ2VNYW5hZ2VyLW9ubHksIGV4YWN0bHkgbGlrZSBgaW52YWxpZGF0ZWA6IHRoZSBjbGFpbSB3aW5kb3cgaXMKdGhlIENoYWxsZW5nZU1hbmFnZXIncyBjb25jZXB0IGFuZCBub2JvZHkgZWxzZSBtYXkgb3BlbiBvciBjbG9zZSBvbmUuClRoZSBmcmVlemUgaXMgZXhwcmVzc2VkIGFzIGEgc2V0dGxlbWVudCBkZWFkbGluZSByYXRoZXIgdGhhbiBhIHNlcGFyYXRlCmZsYWcgc28gdGhhdCBib3RoIG1vbmV5IGNvbnRyYWN0cyBnZXQgaXQgZm9yIGZyZWUg4oCUIHRoZXkgYWxyZWFkeSByZWZ1c2UKdG8gcmVsZWFzZSBiZWZvcmUgYGdldF9jZXJ0X3NldHRsZW1lbnRfZGVhZGxpbmVgLgAAAAAAABBzZXRfY2xhaW1fZnJlZXplAAAAAgAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAV1bnRpbAAAAAAAAAYAAAAA",
        "AAAAAAAAAAAAAAARZ2V0X2NlcnRfb3BlcmF0b3IAAAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAAEw==",
        "AAAAAAAAAZlXaGVuIHRoZSBjZXJ0aWZpY2F0ZSB3YXMgcHVibGlzaGVkLgoKVGhlIFByZW1pdW1WYXVsdCByZWFkcyB0aGlzIHRvZ2V0aGVyIHdpdGggYGdldF9jZXJ0X2V4cGlyZXNfYXRgIHRvIHByaWNlCmNvdmVyYWdlIG92ZXIgYGV4cGlyZXNfYXQgLSBpc3N1ZWRfYXRgLiBCb3RoIGZpZWxkcyBhcmUgaW1tdXRhYmxlIG9uY2UKcHVibGlzaGVkLCB3aGljaCBpcyB0aGUgcG9pbnQ6IGEgcHJlbWl1bSBwcmljZWQgZnJvbSBgbm93YCB3b3VsZCBiZSBhCmZ1bmN0aW9uIG9mIHdoZW4gdGhlIG9wZXJhdG9yIGNob3NlIHRvIHBheSBpdCwgYW5kIGFuIG9wZXJhdG9yIHdvdWxkCnNpbXBseSB3YWl0IHVudGlsIHRoZSBpbnN0YW50IGJlZm9yZSBleHBpcnkgYW5kIGJ1eSBhIHllYXIgb2YgY292ZXJhZ2UKZm9yIGEgZGF5J3MgcHJpY2UuAAAAAAAAEmdldF9jZXJ0X2lzc3VlZF9hdAAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAY=",
        "AAAAAAAAAAAAAAATZ2V0X2NlcnRfZXhwaXJlc19hdAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAABg==",
        "AAAAAAAAAQRUaGUgaW5zdGFudCBhZnRlciB3aGljaCBub3RoaW5nIGNhbiBzdGlsbCBiZSBwcm92ZW4gYWdhaW5zdCB0aGlzCmNlcnRpZmljYXRlLCBhbmQgdGhlcmVmb3JlIHRoZSBpbnN0YW50IGl0cyBjb2xsYXRlcmFsIG1heSB1bndpbmQuCgpCb3RoIHRoZSBSZXNlcnZlVmF1bHQgKG9wZXJhdG9yJ3MgcmVzZXJ2ZSkgYW5kIEF1ZGl0b3JTdGFraW5nIChhdWRpdG9yJ3MKYWxsb2NhdGlvbikgbG9jayB0byB0aGlzLCBzbyB0aGUgdHdvIG5ldmVyIGRyaWZ0IGFwYXJ0LgAAABxnZXRfY2VydF9zZXR0bGVtZW50X2RlYWRsaW5lAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAY=" ]),
      options
    )
  }
  public readonly fromJSON = {
    attest: this.txFromJSON<null>,
        verify: this.txFromJSON<VerifyResult>,
        publish: this.txFromJSON<u64>,
        is_frozen: this.txFromJSON<boolean>,
        initialize: this.txFromJSON<null>,
        invalidate: this.txFromJSON<null>,
        get_cert_id: this.txFromJSON<u64>,
        get_cert_agent: this.txFromJSON<string>,
        get_cert_bound: this.txFromJSON<i128>,
        get_cert_count: this.txFromJSON<u64>,
        get_certificate: this.txFromJSON<Certificate>,
        get_cert_auditor: this.txFromJSON<string>,
        get_cert_reserve: this.txFromJSON<i128>,
        get_claim_freeze: this.txFromJSON<u64>,
        is_cert_verified: this.txFromJSON<boolean>,
        set_claim_freeze: this.txFromJSON<null>,
        get_cert_operator: this.txFromJSON<string>,
        get_cert_issued_at: this.txFromJSON<u64>,
        get_cert_expires_at: this.txFromJSON<u64>,
        get_cert_settlement_deadline: this.txFromJSON<u64>
  }
}