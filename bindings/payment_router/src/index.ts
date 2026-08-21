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
    contractId: "CA5OPBLVGNPAFNFHY3ATNZXJ72L3K4EG4RIQYOWOKESAN7C75DDMMCMJ",
  }
} as const

export type DataKey = {tag: "Registry", values: void} | {tag: "Token", values: void} | {tag: "Supply", values: void} | {tag: "Balance", values: readonly [string]} | {tag: "Allowance", values: readonly [AllowanceKey]} | {tag: "Enrolled", values: readonly [string]} | {tag: "Cert", values: readonly [u64]} | {tag: "Float", values: readonly [u64]} | {tag: "Spent", values: readonly [u64]} | {tag: "PostExpiry", values: readonly [u64]};


export interface CertConfig {
  /**
 * §6 float cap: the most underlying USDC the router will accept into this
 * certificate. This is the number that bounds what a stolen agent key can
 * reach — a thief cannot spend float the operator never deposited.
 */
float_cap: i128;
  /**
 * §6 kill switch. Operator-only, both directions.
 */
halted: boolean;
  /**
 * The operator named on the certificate, snapshotted at first enrollment.
 * Only this address may halt, resume, or change the cap.
 */
operator: string;
}


/**
 * The hot-path copy of an agent's certificate.
 * 
 * `expires_at` is snapshotted at `enroll` so that `transfer` can decide whether
 * a payment is post-expiry without invoking the Registry. Certificate expiry is
 * immutable in the Registry once published, so the snapshot cannot go stale.
 */
export interface Enrollment {
  cert_id: u64;
  expires_at: u64;
}


/**
 * Everything a `spent > bound`-style predicate needs in order to apply §7's
 * grace window and de-minimis floor **later**. This contract records; it does
 * not judge. Applying the window or the floor here would bake two contested
 * parameters into a redeploy-only surface.
 */
export interface PostExpiry {
  /**
 * Number of post-expiry payments.
 */
count: u32;
  /**
 * When the first post-expiry payment settled — the timestamp a grace window
 * is measured against.
 */
first_at: u64;
  /**
 * The largest single post-expiry payment, and when it settled — the pair a
 * de-minimis floor is applied to.
 */
max_payment: i128;
  max_payment_at: u64;
  /**
 * Cumulative value routed strictly after `expires_at`.
 */
total: i128;
}


export interface AllowanceKey {
  from: string;
  spender: string;
}


export interface AllowanceValue {
  amount: i128;
  expiration_ledger: u32;
}

export interface Client {
  /**
   * Construct and simulate a burn transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  burn: ({from, amount}: {from: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a halt transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Halt routing for a certificate. Operator-only, and deliberately
   * independent of the challenge system: compromise response must not wait on
   * a challenge, and halting neither invalidates the certificate nor exposes
   * the auditor to a slash.
   * 
   * While halted, no enrolled agent of this certificate can move value —
   * `transfer`, `transfer_from`, `burn`, `burn_from` and `withdraw` are all
   * refused. `transfer_from` is included because otherwise an allowance the
   * thief granted themselves *before* the halt would survive it and keep
   * draining the float, which would defeat the point of halting first.
   * Allowances are left recorded, not deleted, so a halt/resume cycle does
   * not silently destroy legitimate standing approvals.
   * 
   * The agent key cannot clear this: `resume` authenticates against the
   * certificate's operator, which is exactly the address a thief holding the
   * agent key does not have.
   * 
   * Halting freezes the float rather than rescuing it — `withdraw` is gated
   * too, so the honest operator cannot get their own money out either. See
   * `
   */
  halt: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a name transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  name: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a float transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  float: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a spent transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Cumulative **gross flow** routed by this certificate. See the header:
   * this is not a measure of loss and must never size a payout on its own.
   */
  spent: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a enroll transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Bind `agent` to `cert_id` and set that certificate's float cap.
   * 
   * **Both the agent and the operator must authorize, and neither alone is
   * enough.**
   * 
   * The operator, because enrollment attaches spend to *their* certificate:
   * the counter it feeds is the evidence a challenge will read, and the float
   * cap it sets is a claim about their own collateral. If the agent could
   * enroll alone, anyone could bind an address they control to a stranger's
   * certificate and manufacture a `spent > bound` record against them.
   * 
   * The agent, because enrollment is not free to the agent either: it
   * subjects every one of that address's transfers to metering and puts the
   * address under the operator's kill switch. You may not conscript an
   * address you do not control. If the operator could enroll alone, they
   * could freeze an unrelated party's balance by halting.
   * 
   * The operator address is read live from the Registry, so authority follows
   * the certificate rather than a local admin field.
   */
  enroll: ({agent, cert_id, float_cap}: {agent: string, cert_id: u64, float_cap: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a resume transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  resume: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a symbol transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  symbol: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a approve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  approve: ({from, spender, amount, expiration_ledger}: {from: string, spender: string, amount: i128, expiration_ledger: u32}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a balance transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  balance: ({id}: {id: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a cert_of transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The certificate an agent is bound to, or `None` if untracked.
   */
  cert_of: ({agent}: {agent: string}, options?: MethodOptions) => Promise<AssembledTransaction<Option<u64>>>

  /**
   * Construct and simulate a deposit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Move underlying USDC into custody and credit an equal internal balance.
   * 
   * This one MAY call the SAC — only `transfer` is constrained by x402.
   */
  deposit: ({from, amount}: {from: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a clawback transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Sweep a halted certificate's agent balance back to the operator (§6).
   * 
   * ## The problem this exists to solve
   * 
   * `halt` gates `transfer`, `transfer_from`, `burn`, `burn_from` **and**
   * `withdraw`. `withdraw` has to be gated, or a thief holding the agent key
   * simply withdraws the float and the kill switch is decorative. But that
   * gate is symmetric: it also stops the *honest operator* from recovering
   * their own float, and the only way out was `resume` — which re-arms the
   * thief. The operator was made to choose between losing the money and
   * handing it back to the attacker. `clawback` is the third door.
   * 
   * ## Why this is safe
   * 
   * Two independent reasons, and both matter:
   * 
   * 1. **The thief cannot call it.** Authority is the certificate's
   * **operator**, read live from the Registry at enrollment and stored in
   * `CertConfig` — exactly the address a thief holding the *agent* key
   * does not have. It is the same authority that gates `halt`, `resume`
   * and `set_float_cap`.
   * 2. **The money cannot reach a stranger.** The destination is not a
   * pa
   */
  clawback: ({cert_id, agent}: {cert_id: u64, agent: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a decimals transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  decimals: (options?: MethodOptions) => Promise<AssembledTransaction<u32>>

  /**
   * Construct and simulate a transfer transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The x402 hot path.
   * 
   * One `require_auth`, an internal balance move, the meter, and exactly one
   * `transfer` event. No cross-contract call happens here and none may ever
   * be added — see the header.
   */
  transfer: ({from, to, amount}: {from: string, to: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a withdraw transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Burn internal balance and release the underlying USDC back to its holder.
   */
  withdraw: ({to, amount}: {to: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a allowance transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  allowance: ({from, spender}: {from: string, spender: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a burn_from transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  burn_from: ({spender, from, amount}: {spender: string, from: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a float_cap transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  float_cap: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a is_halted transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  is_halted: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({registry, token}: {registry: string, token: string}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a is_tracked transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  is_tracked: ({agent}: {agent: string}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a total_supply transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Total internal supply. Invariant, asserted in the tests: this equals the
   * router's balance of the underlying USDC. Custody is never fractional.
   */
  total_supply: (options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a set_float_cap transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Operator-only. Lowering the cap does not claw back float already held; it
   * only refuses further deposits.
   */
  set_float_cap: ({cert_id, float_cap}: {cert_id: u64, float_cap: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a transfer_from transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  transfer_from: ({spender, from, to, amount}: {spender: string, from: string, to: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a post_expiry_spent transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Gross flow routed strictly after the certificate's `expires_at`, plus the
   * records §7's grace window and de-minimis floor need. Neither is applied
   * here.
   */
  post_expiry_spent: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<PostExpiry>>

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
      new ContractSpec([ "AAAAAAAAAAAAAAAEYnVybgAAAAIAAAAAAAAABGZyb20AAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAA",
        "AAAAAAAABABIYWx0IHJvdXRpbmcgZm9yIGEgY2VydGlmaWNhdGUuIE9wZXJhdG9yLW9ubHksIGFuZCBkZWxpYmVyYXRlbHkKaW5kZXBlbmRlbnQgb2YgdGhlIGNoYWxsZW5nZSBzeXN0ZW06IGNvbXByb21pc2UgcmVzcG9uc2UgbXVzdCBub3Qgd2FpdCBvbgphIGNoYWxsZW5nZSwgYW5kIGhhbHRpbmcgbmVpdGhlciBpbnZhbGlkYXRlcyB0aGUgY2VydGlmaWNhdGUgbm9yIGV4cG9zZXMKdGhlIGF1ZGl0b3IgdG8gYSBzbGFzaC4KCldoaWxlIGhhbHRlZCwgbm8gZW5yb2xsZWQgYWdlbnQgb2YgdGhpcyBjZXJ0aWZpY2F0ZSBjYW4gbW92ZSB2YWx1ZSDigJQKYHRyYW5zZmVyYCwgYHRyYW5zZmVyX2Zyb21gLCBgYnVybmAsIGBidXJuX2Zyb21gIGFuZCBgd2l0aGRyYXdgIGFyZSBhbGwKcmVmdXNlZC4gYHRyYW5zZmVyX2Zyb21gIGlzIGluY2x1ZGVkIGJlY2F1c2Ugb3RoZXJ3aXNlIGFuIGFsbG93YW5jZSB0aGUKdGhpZWYgZ3JhbnRlZCB0aGVtc2VsdmVzICpiZWZvcmUqIHRoZSBoYWx0IHdvdWxkIHN1cnZpdmUgaXQgYW5kIGtlZXAKZHJhaW5pbmcgdGhlIGZsb2F0LCB3aGljaCB3b3VsZCBkZWZlYXQgdGhlIHBvaW50IG9mIGhhbHRpbmcgZmlyc3QuCkFsbG93YW5jZXMgYXJlIGxlZnQgcmVjb3JkZWQsIG5vdCBkZWxldGVkLCBzbyBhIGhhbHQvcmVzdW1lIGN5Y2xlIGRvZXMKbm90IHNpbGVudGx5IGRlc3Ryb3kgbGVnaXRpbWF0ZSBzdGFuZGluZyBhcHByb3ZhbHMuCgpUaGUgYWdlbnQga2V5IGNhbm5vdCBjbGVhciB0aGlzOiBgcmVzdW1lYCBhdXRoZW50aWNhdGVzIGFnYWluc3QgdGhlCmNlcnRpZmljYXRlJ3Mgb3BlcmF0b3IsIHdoaWNoIGlzIGV4YWN0bHkgdGhlIGFkZHJlc3MgYSB0aGllZiBob2xkaW5nIHRoZQphZ2VudCBrZXkgZG9lcyBub3QgaGF2ZS4KCkhhbHRpbmcgZnJlZXplcyB0aGUgZmxvYXQgcmF0aGVyIHRoYW4gcmVzY3VpbmcgaXQg4oCUIGB3aXRoZHJhd2AgaXMgZ2F0ZWQKdG9vLCBzbyB0aGUgaG9uZXN0IG9wZXJhdG9yIGNhbm5vdCBnZXQgdGhlaXIgb3duIG1vbmV5IG91dCBlaXRoZXIuIFNlZQpgAAAABGhhbHQAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAAA",
        "AAAAAAAAAAAAAAAEbmFtZQAAAAAAAAABAAAAEA==",
        "AAAAAAAAAAAAAAAFZmxvYXQAAAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAACw==",
        "AAAAAAAAAIxDdW11bGF0aXZlICoqZ3Jvc3MgZmxvdyoqIHJvdXRlZCBieSB0aGlzIGNlcnRpZmljYXRlLiBTZWUgdGhlIGhlYWRlcjoKdGhpcyBpcyBub3QgYSBtZWFzdXJlIG9mIGxvc3MgYW5kIG11c3QgbmV2ZXIgc2l6ZSBhIHBheW91dCBvbiBpdHMgb3duLgAAAAVzcGVudAAAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAL",
        "AAAAAAAAA7pCaW5kIGBhZ2VudGAgdG8gYGNlcnRfaWRgIGFuZCBzZXQgdGhhdCBjZXJ0aWZpY2F0ZSdzIGZsb2F0IGNhcC4KCioqQm90aCB0aGUgYWdlbnQgYW5kIHRoZSBvcGVyYXRvciBtdXN0IGF1dGhvcml6ZSwgYW5kIG5laXRoZXIgYWxvbmUgaXMKZW5vdWdoLioqCgpUaGUgb3BlcmF0b3IsIGJlY2F1c2UgZW5yb2xsbWVudCBhdHRhY2hlcyBzcGVuZCB0byAqdGhlaXIqIGNlcnRpZmljYXRlOgp0aGUgY291bnRlciBpdCBmZWVkcyBpcyB0aGUgZXZpZGVuY2UgYSBjaGFsbGVuZ2Ugd2lsbCByZWFkLCBhbmQgdGhlIGZsb2F0CmNhcCBpdCBzZXRzIGlzIGEgY2xhaW0gYWJvdXQgdGhlaXIgb3duIGNvbGxhdGVyYWwuIElmIHRoZSBhZ2VudCBjb3VsZAplbnJvbGwgYWxvbmUsIGFueW9uZSBjb3VsZCBiaW5kIGFuIGFkZHJlc3MgdGhleSBjb250cm9sIHRvIGEgc3RyYW5nZXIncwpjZXJ0aWZpY2F0ZSBhbmQgbWFudWZhY3R1cmUgYSBgc3BlbnQgPiBib3VuZGAgcmVjb3JkIGFnYWluc3QgdGhlbS4KClRoZSBhZ2VudCwgYmVjYXVzZSBlbnJvbGxtZW50IGlzIG5vdCBmcmVlIHRvIHRoZSBhZ2VudCBlaXRoZXI6IGl0CnN1YmplY3RzIGV2ZXJ5IG9uZSBvZiB0aGF0IGFkZHJlc3MncyB0cmFuc2ZlcnMgdG8gbWV0ZXJpbmcgYW5kIHB1dHMgdGhlCmFkZHJlc3MgdW5kZXIgdGhlIG9wZXJhdG9yJ3Mga2lsbCBzd2l0Y2guIFlvdSBtYXkgbm90IGNvbnNjcmlwdCBhbgphZGRyZXNzIHlvdSBkbyBub3QgY29udHJvbC4gSWYgdGhlIG9wZXJhdG9yIGNvdWxkIGVucm9sbCBhbG9uZSwgdGhleQpjb3VsZCBmcmVlemUgYW4gdW5yZWxhdGVkIHBhcnR5J3MgYmFsYW5jZSBieSBoYWx0aW5nLgoKVGhlIG9wZXJhdG9yIGFkZHJlc3MgaXMgcmVhZCBsaXZlIGZyb20gdGhlIFJlZ2lzdHJ5LCBzbyBhdXRob3JpdHkgZm9sbG93cwp0aGUgY2VydGlmaWNhdGUgcmF0aGVyIHRoYW4gYSBsb2NhbCBhZG1pbiBmaWVsZC4AAAAAAAZlbnJvbGwAAAAAAAMAAAAAAAAABWFnZW50AAAAAAAAEwAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAlmbG9hdF9jYXAAAAAAAAALAAAAAA==",
        "AAAAAAAAAAAAAAAGcmVzdW1lAAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAAA",
        "AAAAAAAAAAAAAAAGc3ltYm9sAAAAAAAAAAAAAQAAABA=",
        "AAAAAAAAAAAAAAAHYXBwcm92ZQAAAAAEAAAAAAAAAARmcm9tAAAAEwAAAAAAAAAHc3BlbmRlcgAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAEWV4cGlyYXRpb25fbGVkZ2VyAAAAAAAABAAAAAA=",
        "AAAAAAAAAAAAAAAHYmFsYW5jZQAAAAABAAAAAAAAAAJpZAAAAAAAEwAAAAEAAAAL",
        "AAAAAAAAAD1UaGUgY2VydGlmaWNhdGUgYW4gYWdlbnQgaXMgYm91bmQgdG8sIG9yIGBOb25lYCBpZiB1bnRyYWNrZWQuAAAAAAAAB2NlcnRfb2YAAAAAAQAAAAAAAAAFYWdlbnQAAAAAAAATAAAAAQAAA+gAAAAG",
        "AAAAAAAAAI5Nb3ZlIHVuZGVybHlpbmcgVVNEQyBpbnRvIGN1c3RvZHkgYW5kIGNyZWRpdCBhbiBlcXVhbCBpbnRlcm5hbCBiYWxhbmNlLgoKVGhpcyBvbmUgTUFZIGNhbGwgdGhlIFNBQyDigJQgb25seSBgdHJhbnNmZXJgIGlzIGNvbnN0cmFpbmVkIGJ5IHg0MDIuAAAAAAAHZGVwb3NpdAAAAAACAAAAAAAAAARmcm9tAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAA==",
        "AAAAAAAABABTd2VlcCBhIGhhbHRlZCBjZXJ0aWZpY2F0ZSdzIGFnZW50IGJhbGFuY2UgYmFjayB0byB0aGUgb3BlcmF0b3IgKMKnNikuCgojIyBUaGUgcHJvYmxlbSB0aGlzIGV4aXN0cyB0byBzb2x2ZQoKYGhhbHRgIGdhdGVzIGB0cmFuc2ZlcmAsIGB0cmFuc2Zlcl9mcm9tYCwgYGJ1cm5gLCBgYnVybl9mcm9tYCAqKmFuZCoqCmB3aXRoZHJhd2AuIGB3aXRoZHJhd2AgaGFzIHRvIGJlIGdhdGVkLCBvciBhIHRoaWVmIGhvbGRpbmcgdGhlIGFnZW50IGtleQpzaW1wbHkgd2l0aGRyYXdzIHRoZSBmbG9hdCBhbmQgdGhlIGtpbGwgc3dpdGNoIGlzIGRlY29yYXRpdmUuIEJ1dCB0aGF0CmdhdGUgaXMgc3ltbWV0cmljOiBpdCBhbHNvIHN0b3BzIHRoZSAqaG9uZXN0IG9wZXJhdG9yKiBmcm9tIHJlY292ZXJpbmcKdGhlaXIgb3duIGZsb2F0LCBhbmQgdGhlIG9ubHkgd2F5IG91dCB3YXMgYHJlc3VtZWAg4oCUIHdoaWNoIHJlLWFybXMgdGhlCnRoaWVmLiBUaGUgb3BlcmF0b3Igd2FzIG1hZGUgdG8gY2hvb3NlIGJldHdlZW4gbG9zaW5nIHRoZSBtb25leSBhbmQKaGFuZGluZyBpdCBiYWNrIHRvIHRoZSBhdHRhY2tlci4gYGNsYXdiYWNrYCBpcyB0aGUgdGhpcmQgZG9vci4KCiMjIFdoeSB0aGlzIGlzIHNhZmUKClR3byBpbmRlcGVuZGVudCByZWFzb25zLCBhbmQgYm90aCBtYXR0ZXI6CgoxLiAqKlRoZSB0aGllZiBjYW5ub3QgY2FsbCBpdC4qKiBBdXRob3JpdHkgaXMgdGhlIGNlcnRpZmljYXRlJ3MKKipvcGVyYXRvcioqLCByZWFkIGxpdmUgZnJvbSB0aGUgUmVnaXN0cnkgYXQgZW5yb2xsbWVudCBhbmQgc3RvcmVkIGluCmBDZXJ0Q29uZmlnYCDigJQgZXhhY3RseSB0aGUgYWRkcmVzcyBhIHRoaWVmIGhvbGRpbmcgdGhlICphZ2VudCoga2V5CmRvZXMgbm90IGhhdmUuIEl0IGlzIHRoZSBzYW1lIGF1dGhvcml0eSB0aGF0IGdhdGVzIGBoYWx0YCwgYHJlc3VtZWAKYW5kIGBzZXRfZmxvYXRfY2FwYC4KMi4gKipUaGUgbW9uZXkgY2Fubm90IHJlYWNoIGEgc3RyYW5nZXIuKiogVGhlIGRlc3RpbmF0aW9uIGlzIG5vdCBhCnBhAAAACGNsYXdiYWNrAAAAAgAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAVhZ2VudAAAAAAAABMAAAABAAAACw==",
        "AAAAAAAAAAAAAAAIZGVjaW1hbHMAAAAAAAAAAQAAAAQ=",
        "AAAAAAAAAMFUaGUgeDQwMiBob3QgcGF0aC4KCk9uZSBgcmVxdWlyZV9hdXRoYCwgYW4gaW50ZXJuYWwgYmFsYW5jZSBtb3ZlLCB0aGUgbWV0ZXIsIGFuZCBleGFjdGx5IG9uZQpgdHJhbnNmZXJgIGV2ZW50LiBObyBjcm9zcy1jb250cmFjdCBjYWxsIGhhcHBlbnMgaGVyZSBhbmQgbm9uZSBtYXkgZXZlcgpiZSBhZGRlZCDigJQgc2VlIHRoZSBoZWFkZXIuAAAAAAAACHRyYW5zZmVyAAAAAwAAAAAAAAAEZnJvbQAAABMAAAAAAAAAAnRvAAAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAA",
        "AAAAAAAAAElCdXJuIGludGVybmFsIGJhbGFuY2UgYW5kIHJlbGVhc2UgdGhlIHVuZGVybHlpbmcgVVNEQyBiYWNrIHRvIGl0cyBob2xkZXIuAAAAAAAACHdpdGhkcmF3AAAAAgAAAAAAAAACdG8AAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAA=",
        "AAAAAAAAAAAAAAAJYWxsb3dhbmNlAAAAAAAAAgAAAAAAAAAEZnJvbQAAABMAAAAAAAAAB3NwZW5kZXIAAAAAEwAAAAEAAAAL",
        "AAAAAAAAAAAAAAAJYnVybl9mcm9tAAAAAAAAAwAAAAAAAAAHc3BlbmRlcgAAAAATAAAAAAAAAARmcm9tAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAA==",
        "AAAAAAAAAAAAAAAJZmxvYXRfY2FwAAAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAs=",
        "AAAAAAAAAAAAAAAJaXNfaGFsdGVkAAAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAE=",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAACgAAAAAAAAAAAAAACFJlZ2lzdHJ5AAAAAAAAAChUaGUgdW5kZXJseWluZyBVU0RDIFNBQyBoZWxkIGluIGN1c3RvZHkuAAAABVRva2VuAAAAAAAAAAAAAENUb3RhbCBpbnRlcm5hbCBzdXBwbHkuIEludmFyaWFudDogZXF1YWxzIHRoZSByb3V0ZXIncyBVU0RDIGJhbGFuY2UuAAAAAAZTdXBwbHkAAAAAAAEAAAAAAAAAB0JhbGFuY2UAAAAAAQAAABMAAAABAAAAAAAAAAlBbGxvd2FuY2UAAAAAAAABAAAH0AAAAAxBbGxvd2FuY2VLZXkAAAABAAAAPGFnZW50IC0+IEVucm9sbG1lbnQuIFByZXNlbmNlIGlzIHdoYXQgInRyYWNrZWQiIG1lYW5zICjCpzgpLgAAAAhFbnJvbGxlZAAAAAEAAAATAAAAAQAAAAAAAAAEQ2VydAAAAAEAAAAGAAAAAQAAAD1VbmRlcmx5aW5nIFVTREMgY3VycmVudGx5IGhlbGQgb24gYmVoYWxmIG9mIHRoaXMgY2VydGlmaWNhdGUuAAAAAAAABUZsb2F0AAAAAAAAAQAAAAYAAAABAAAAREN1bXVsYXRpdmUgZ3Jvc3MgZmxvdyByb3V0ZWQgYnkgdGhpcyBjZXJ0aWZpY2F0ZSdzIGFnZW50cy4gTW9ub3RvbmUuAAAABVNwZW50AAAAAAAAAQAAAAYAAAABAAAAAAAAAApQb3N0RXhwaXJ5AAAAAAABAAAABg==",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAAAgAAAAAAAAAIcmVnaXN0cnkAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAA",
        "AAAAAAAAAAAAAAAKaXNfdHJhY2tlZAAAAAAAAQAAAAAAAAAFYWdlbnQAAAAAAAATAAAAAQAAAAE=",
        "AAAAAAAAAI5Ub3RhbCBpbnRlcm5hbCBzdXBwbHkuIEludmFyaWFudCwgYXNzZXJ0ZWQgaW4gdGhlIHRlc3RzOiB0aGlzIGVxdWFscyB0aGUKcm91dGVyJ3MgYmFsYW5jZSBvZiB0aGUgdW5kZXJseWluZyBVU0RDLiBDdXN0b2R5IGlzIG5ldmVyIGZyYWN0aW9uYWwuAAAAAAAMdG90YWxfc3VwcGx5AAAAAAAAAAEAAAAL",
        "AAAAAQAAAAAAAAAAAAAACkNlcnRDb25maWcAAAAAAAMAAADTwqc2IGZsb2F0IGNhcDogdGhlIG1vc3QgdW5kZXJseWluZyBVU0RDIHRoZSByb3V0ZXIgd2lsbCBhY2NlcHQgaW50byB0aGlzCmNlcnRpZmljYXRlLiBUaGlzIGlzIHRoZSBudW1iZXIgdGhhdCBib3VuZHMgd2hhdCBhIHN0b2xlbiBhZ2VudCBrZXkgY2FuCnJlYWNoIOKAlCBhIHRoaWVmIGNhbm5vdCBzcGVuZCBmbG9hdCB0aGUgb3BlcmF0b3IgbmV2ZXIgZGVwb3NpdGVkLgAAAAAJZmxvYXRfY2FwAAAAAAAACwAAADDCpzYga2lsbCBzd2l0Y2guIE9wZXJhdG9yLW9ubHksIGJvdGggZGlyZWN0aW9ucy4AAAAGaGFsdGVkAAAAAAABAAAAflRoZSBvcGVyYXRvciBuYW1lZCBvbiB0aGUgY2VydGlmaWNhdGUsIHNuYXBzaG90dGVkIGF0IGZpcnN0IGVucm9sbG1lbnQuCk9ubHkgdGhpcyBhZGRyZXNzIG1heSBoYWx0LCByZXN1bWUsIG9yIGNoYW5nZSB0aGUgY2FwLgAAAAAACG9wZXJhdG9yAAAAEw==",
        "AAAAAQAAARRUaGUgaG90LXBhdGggY29weSBvZiBhbiBhZ2VudCdzIGNlcnRpZmljYXRlLgoKYGV4cGlyZXNfYXRgIGlzIHNuYXBzaG90dGVkIGF0IGBlbnJvbGxgIHNvIHRoYXQgYHRyYW5zZmVyYCBjYW4gZGVjaWRlIHdoZXRoZXIKYSBwYXltZW50IGlzIHBvc3QtZXhwaXJ5IHdpdGhvdXQgaW52b2tpbmcgdGhlIFJlZ2lzdHJ5LiBDZXJ0aWZpY2F0ZSBleHBpcnkgaXMKaW1tdXRhYmxlIGluIHRoZSBSZWdpc3RyeSBvbmNlIHB1Ymxpc2hlZCwgc28gdGhlIHNuYXBzaG90IGNhbm5vdCBnbyBzdGFsZS4AAAAAAAAACkVucm9sbG1lbnQAAAAAAAIAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAAAAAAKZXhwaXJlc19hdAAAAAAABg==",
        "AAAAAQAAAQlFdmVyeXRoaW5nIGEgYHNwZW50ID4gYm91bmRgLXN0eWxlIHByZWRpY2F0ZSBuZWVkcyBpbiBvcmRlciB0byBhcHBseSDCpzcncwpncmFjZSB3aW5kb3cgYW5kIGRlLW1pbmltaXMgZmxvb3IgKipsYXRlcioqLiBUaGlzIGNvbnRyYWN0IHJlY29yZHM7IGl0IGRvZXMKbm90IGp1ZGdlLiBBcHBseWluZyB0aGUgd2luZG93IG9yIHRoZSBmbG9vciBoZXJlIHdvdWxkIGJha2UgdHdvIGNvbnRlc3RlZApwYXJhbWV0ZXJzIGludG8gYSByZWRlcGxveS1vbmx5IHN1cmZhY2UuAAAAAAAAAAAAAApQb3N0RXhwaXJ5AAAAAAAFAAAAH051bWJlciBvZiBwb3N0LWV4cGlyeSBwYXltZW50cy4AAAAABWNvdW50AAAAAAAABAAAAGBXaGVuIHRoZSBmaXJzdCBwb3N0LWV4cGlyeSBwYXltZW50IHNldHRsZWQg4oCUIHRoZSB0aW1lc3RhbXAgYSBncmFjZSB3aW5kb3cKaXMgbWVhc3VyZWQgYWdhaW5zdC4AAAAIZmlyc3RfYXQAAAAGAAAAalRoZSBsYXJnZXN0IHNpbmdsZSBwb3N0LWV4cGlyeSBwYXltZW50LCBhbmQgd2hlbiBpdCBzZXR0bGVkIOKAlCB0aGUgcGFpciBhCmRlLW1pbmltaXMgZmxvb3IgaXMgYXBwbGllZCB0by4AAAAAAAttYXhfcGF5bWVudAAAAAALAAAAAAAAAA5tYXhfcGF5bWVudF9hdAAAAAAABgAAADRDdW11bGF0aXZlIHZhbHVlIHJvdXRlZCBzdHJpY3RseSBhZnRlciBgZXhwaXJlc19hdGAuAAAABXRvdGFsAAAAAAAACw==",
        "AAAAAAAAAGhPcGVyYXRvci1vbmx5LiBMb3dlcmluZyB0aGUgY2FwIGRvZXMgbm90IGNsYXcgYmFjayBmbG9hdCBhbHJlYWR5IGhlbGQ7IGl0Cm9ubHkgcmVmdXNlcyBmdXJ0aGVyIGRlcG9zaXRzLgAAAA1zZXRfZmxvYXRfY2FwAAAAAAAAAgAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAlmbG9hdF9jYXAAAAAAAAALAAAAAA==",
        "AAAAAAAAAAAAAAANdHJhbnNmZXJfZnJvbQAAAAAAAAQAAAAAAAAAB3NwZW5kZXIAAAAAEwAAAAAAAAAEZnJvbQAAABMAAAAAAAAAAnRvAAAAAAATAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAA",
        "AAAAAQAAAAAAAAAAAAAADEFsbG93YW5jZUtleQAAAAIAAAAAAAAABGZyb20AAAATAAAAAAAAAAdzcGVuZGVyAAAAABM=",
        "AAAAAQAAAAAAAAAAAAAADkFsbG93YW5jZVZhbHVlAAAAAAACAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAAEWV4cGlyYXRpb25fbGVkZ2VyAAAAAAAABA==",
        "AAAAAAAAAJhHcm9zcyBmbG93IHJvdXRlZCBzdHJpY3RseSBhZnRlciB0aGUgY2VydGlmaWNhdGUncyBgZXhwaXJlc19hdGAsIHBsdXMgdGhlCnJlY29yZHMgwqc3J3MgZ3JhY2Ugd2luZG93IGFuZCBkZS1taW5pbWlzIGZsb29yIG5lZWQuIE5laXRoZXIgaXMgYXBwbGllZApoZXJlLgAAABFwb3N0X2V4cGlyeV9zcGVudAAAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAfQAAAAClBvc3RFeHBpcnkAAA==" ]),
      options
    )
  }
  public readonly fromJSON = {
    burn: this.txFromJSON<null>,
        halt: this.txFromJSON<null>,
        name: this.txFromJSON<string>,
        float: this.txFromJSON<i128>,
        spent: this.txFromJSON<i128>,
        enroll: this.txFromJSON<null>,
        resume: this.txFromJSON<null>,
        symbol: this.txFromJSON<string>,
        approve: this.txFromJSON<null>,
        balance: this.txFromJSON<i128>,
        cert_of: this.txFromJSON<Option<u64>>,
        deposit: this.txFromJSON<null>,
        clawback: this.txFromJSON<i128>,
        decimals: this.txFromJSON<u32>,
        transfer: this.txFromJSON<null>,
        withdraw: this.txFromJSON<null>,
        allowance: this.txFromJSON<i128>,
        burn_from: this.txFromJSON<null>,
        float_cap: this.txFromJSON<i128>,
        is_halted: this.txFromJSON<boolean>,
        initialize: this.txFromJSON<null>,
        is_tracked: this.txFromJSON<boolean>,
        total_supply: this.txFromJSON<i128>,
        set_float_cap: this.txFromJSON<null>,
        transfer_from: this.txFromJSON<null>,
        post_expiry_spent: this.txFromJSON<PostExpiry>
  }
}