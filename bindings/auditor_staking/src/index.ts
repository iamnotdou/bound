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
    contractId: "CBPUCSASKMQKRWJ7WUTQ6KUPX66QUBV2E6V4CQ46BI4ZLJSW6AFPR6RB",
  }
} as const

export type DataKey = {tag: "ChallengeManager", values: void} | {tag: "Registry", values: void} | {tag: "Token", values: void} | {tag: "MinRegistrationStake", values: void} | {tag: "Stake", values: readonly [string]} | {tag: "Allocated", values: readonly [string]} | {tag: "Allocation", values: readonly [u64]} | {tag: "AllocationAuditor", values: readonly [u64]} | {tag: "AllocationUnlockAt", values: readonly [u64]} | {tag: "LockedUntil", values: readonly [string]};

export interface Client {
  /**
   * Construct and simulate a stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Deposit USDC into the staking contract. Deposited capital starts entirely
   * free; it only goes at risk when it is allocated to a certificate.
   */
  stake: ({auditor, amount}: {auditor: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a release transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Withdraw **free** stake. Allocated capital is untouchable by definition:
   * it is locked because a live certificate stands on it, not because of a
   * timestamp on the auditor.
   */
  release: ({auditor}: {auditor: string}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a allocate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Allocate `amount` of an auditor's free stake to one certificate, locked
   * until `until`. Only the Registry may call this — it does so inside
   * `attest`, so the moment an auditor vouches, a named slice of their
   * capital is at risk for that certificate and cannot be pulled out from
   * under the counterparty.
   * 
   * `until` is the certificate's **settlement deadline**
   * (`expires_at + CHALLENGE_WINDOW`), not its expiry. A proof about
   * post-expiry activity only becomes provable after expiry; if the
   * allocation unlocked at `expires_at` the proof would settle against an
   * already-freed allocation every single time.
   */
  allocate: ({auditor, cert_id, amount, until}: {auditor: string, cert_id: u64, amount: i128, until: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Total capital custodied for this auditor: free + allocated.
   */
  get_stake: ({auditor}: {auditor: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({challenge_manager, registry, token, min_stake}: {challenge_manager: string, registry: string, token: string, min_stake: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a locked_until transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Informational: the latest settlement deadline across this auditor's
   * allocations. Nothing gates on it — an allocation is locked because it is
   * allocated, not because of this timestamp.
   */
  locked_until: ({auditor}: {auditor: string}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_allocated transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_allocated: ({auditor}: {auditor: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_min_stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_min_stake: (options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a is_registered transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Registration is judged on **free** stake, because the thing an auditor
   * needs before vouching for one more certificate is capital that is not
   * already vouching for another one. Under the old global-stake model this
   * read the whole book, so a single $500 stake could back an unlimited
   * number of certificates at $500 of advertised collateral each.
   */
  is_registered: ({auditor}: {auditor: string}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a get_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_allocation: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_free_stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Capital not currently standing behind any certificate. This is what can
   * be allocated to a new attestation or withdrawn.
   */
  get_free_stake: ({auditor}: {auditor: string}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a slash_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Slash a certificate's allocation to the **treasury**, and nowhere else.
   * 
   * The recipient is a parameter only so the ChallengeManager can pass the
   * treasury address it was initialized with; there is deliberately no path
   * that lets a challenger, a victim or an operator name themselves here.
   * Slashed stake must never be a prize, or manufacturing a true proof
   * becomes a business model.
   * 
   * The draw is capped by *this certificate's* allocation, so slashing
   * certificate A cannot touch the allocation backing certificate B.
   */
  slash_allocation: ({cert_id, treasury, amount}: {cert_id: u64, treasury: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a retire_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Retire an allocation at settlement: whatever was not slashed goes back
   * to the auditor's **free** stake.
   * 
   * Without this the unslashed remainder would sit allocated to a dead
   * certificate forever — capital stranded, which is the exact defect the
   * per-certificate refactor exists to remove.
   */
  retire_allocation: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a release_allocation transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The auditor frees a live allocation themselves, once the certificate's
   * settlement deadline has passed and no challenge can still land on it.
   */
  release_allocation: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a allocation_unlock_at transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  allocation_unlock_at: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_allocation_auditor transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_allocation_auditor: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Option<string>>>

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
      new ContractSpec([ "AAAAAAAAAItEZXBvc2l0IFVTREMgaW50byB0aGUgc3Rha2luZyBjb250cmFjdC4gRGVwb3NpdGVkIGNhcGl0YWwgc3RhcnRzIGVudGlyZWx5CmZyZWU7IGl0IG9ubHkgZ29lcyBhdCByaXNrIHdoZW4gaXQgaXMgYWxsb2NhdGVkIHRvIGEgY2VydGlmaWNhdGUuAAAAAAVzdGFrZQAAAAAAAAIAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAA==",
        "AAAAAAAAAKlXaXRoZHJhdyAqKmZyZWUqKiBzdGFrZS4gQWxsb2NhdGVkIGNhcGl0YWwgaXMgdW50b3VjaGFibGUgYnkgZGVmaW5pdGlvbjoKaXQgaXMgbG9ja2VkIGJlY2F1c2UgYSBsaXZlIGNlcnRpZmljYXRlIHN0YW5kcyBvbiBpdCwgbm90IGJlY2F1c2Ugb2YgYQp0aW1lc3RhbXAgb24gdGhlIGF1ZGl0b3IuAAAAAAAAB3JlbGVhc2UAAAAAAQAAAAAAAAAHYXVkaXRvcgAAAAATAAAAAA==",
        "AAAAAAAAAlZBbGxvY2F0ZSBgYW1vdW50YCBvZiBhbiBhdWRpdG9yJ3MgZnJlZSBzdGFrZSB0byBvbmUgY2VydGlmaWNhdGUsIGxvY2tlZAp1bnRpbCBgdW50aWxgLiBPbmx5IHRoZSBSZWdpc3RyeSBtYXkgY2FsbCB0aGlzIOKAlCBpdCBkb2VzIHNvIGluc2lkZQpgYXR0ZXN0YCwgc28gdGhlIG1vbWVudCBhbiBhdWRpdG9yIHZvdWNoZXMsIGEgbmFtZWQgc2xpY2Ugb2YgdGhlaXIKY2FwaXRhbCBpcyBhdCByaXNrIGZvciB0aGF0IGNlcnRpZmljYXRlIGFuZCBjYW5ub3QgYmUgcHVsbGVkIG91dCBmcm9tCnVuZGVyIHRoZSBjb3VudGVycGFydHkuCgpgdW50aWxgIGlzIHRoZSBjZXJ0aWZpY2F0ZSdzICoqc2V0dGxlbWVudCBkZWFkbGluZSoqCihgZXhwaXJlc19hdCArIENIQUxMRU5HRV9XSU5ET1dgKSwgbm90IGl0cyBleHBpcnkuIEEgcHJvb2YgYWJvdXQKcG9zdC1leHBpcnkgYWN0aXZpdHkgb25seSBiZWNvbWVzIHByb3ZhYmxlIGFmdGVyIGV4cGlyeTsgaWYgdGhlCmFsbG9jYXRpb24gdW5sb2NrZWQgYXQgYGV4cGlyZXNfYXRgIHRoZSBwcm9vZiB3b3VsZCBzZXR0bGUgYWdhaW5zdCBhbgphbHJlYWR5LWZyZWVkIGFsbG9jYXRpb24gZXZlcnkgc2luZ2xlIHRpbWUuAAAAAAAIYWxsb2NhdGUAAAAEAAAAAAAAAAdhdWRpdG9yAAAAABMAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAAAAAAGYW1vdW50AAAAAAALAAAAAAAAAAV1bnRpbAAAAAAAAAYAAAAA",
        "AAAAAAAAADtUb3RhbCBjYXBpdGFsIGN1c3RvZGllZCBmb3IgdGhpcyBhdWRpdG9yOiBmcmVlICsgYWxsb2NhdGVkLgAAAAAJZ2V0X3N0YWtlAAAAAAAAAQAAAAAAAAAHYXVkaXRvcgAAAAATAAAAAQAAAAs=",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAACgAAAAAAAAAAAAAAEENoYWxsZW5nZU1hbmFnZXIAAAAAAAAAAAAAAAhSZWdpc3RyeQAAAAAAAAAAAAAABVRva2VuAAAAAAAAAAAAAAAAAAAUTWluUmVnaXN0cmF0aW9uU3Rha2UAAAABAAAAeFRvdGFsIGNhcGl0YWwgdGhpcyBhdWRpdG9yIGhhcyBkZXBvc2l0ZWQ6IGZyZWUgKyBhbGxvY2F0ZWQuIFRoaXMgaXMgdGhlCm51bWJlciB0aGUgY29udHJhY3QgYWN0dWFsbHkgY3VzdG9kaWVzIGZvciB0aGVtLgAAAAVTdGFrZQAAAAAAAAEAAAATAAAAAQAAAKdTdW0gb2YgZXZlcnkgbGl2ZSBwZXItY2VydGlmaWNhdGUgYWxsb2NhdGlvbiB0aGlzIGF1ZGl0b3IgaG9sZHMuCmBmcmVlID0gU3Rha2UgLSBBbGxvY2F0ZWRgLCBhbmQgZnJlZSBzdGFrZSBpcyB3aGF0IG1heSBiZSBhbGxvY2F0ZWQgdG8gYQpuZXcgY2VydGlmaWNhdGUgb3Igd2l0aGRyYXduLgAAAAAJQWxsb2NhdGVkAAAAAAAAAQAAABMAAAABAAAA/VBlci1jZXJ0aWZpY2F0ZSBhbGxvY2F0aW9uLCBpbiB0aGUgc3R5bGUgdGhlIFJlc2VydmVWYXVsdCBhZG9wdGVkOiB0aGUKc2xpY2Ugb2YgYW4gYXVkaXRvcidzIHN0YWtlIHRoYXQgc3RhbmRzIGJlaGluZCBleGFjdGx5IG9uZSBjZXJ0aWZpY2F0ZS4KQSBzbGFzaCBkcmF3cyBhZ2FpbnN0IHRoaXMgYW5kIG5vdGhpbmcgZWxzZSwgc28gb25lIGJhZCBjZXJ0aWZpY2F0ZSBjYW4KbmV2ZXIgZGVzdHJveSBhbiBhdWRpdG9yJ3Mgd2hvbGUgYm9vay4AAAAAAAAKQWxsb2NhdGlvbgAAAAAAAQAAAAYAAAABAAAAAAAAABFBbGxvY2F0aW9uQXVkaXRvcgAAAAAAAAEAAAAGAAAAAQAAAN9MZWRnZXIgdGltZSBhdCB3aGljaCB0aGlzIGFsbG9jYXRpb24gbWF5IGJlIGZyZWVkIGJ5IHRoZSBhdWRpdG9yLiBTZXQgYnkKdGhlIFJlZ2lzdHJ5IG9uIGF0dGVzdCB0byB0aGUgY2VydGlmaWNhdGUncyAqc2V0dGxlbWVudCBkZWFkbGluZSoKKGBleHBpcmVzX2F0ICsgQ0hBTExFTkdFX1dJTkRPV2ApLCBub3QgaXRzIGV4cGlyeSDigJQgc2VlIHRoZSBjb21tZW50IG9uCmBhbGxvY2F0ZWAuAAAAABJBbGxvY2F0aW9uVW5sb2NrQXQAAAAAAAEAAAAGAAAAAQAAAEhJbmZvcm1hdGlvbmFsOiB0aGUgbGF0ZXN0IHVubG9jayB0aW1lIGFjcm9zcyB0aGlzIGF1ZGl0b3IncyBhbGxvY2F0aW9ucy4AAAALTG9ja2VkVW50aWwAAAAAAQAAABM=",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAABAAAAAAAAAARY2hhbGxlbmdlX21hbmFnZXIAAAAAAAATAAAAAAAAAAhyZWdpc3RyeQAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAJbWluX3N0YWtlAAAAAAAACwAAAAA=",
        "AAAAAAAAALhJbmZvcm1hdGlvbmFsOiB0aGUgbGF0ZXN0IHNldHRsZW1lbnQgZGVhZGxpbmUgYWNyb3NzIHRoaXMgYXVkaXRvcidzCmFsbG9jYXRpb25zLiBOb3RoaW5nIGdhdGVzIG9uIGl0IOKAlCBhbiBhbGxvY2F0aW9uIGlzIGxvY2tlZCBiZWNhdXNlIGl0IGlzCmFsbG9jYXRlZCwgbm90IGJlY2F1c2Ugb2YgdGhpcyB0aW1lc3RhbXAuAAAADGxvY2tlZF91bnRpbAAAAAEAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAEAAAAG",
        "AAAAAAAAAAAAAAANZ2V0X2FsbG9jYXRlZAAAAAAAAAEAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAEAAAAL",
        "AAAAAAAAAAAAAAANZ2V0X21pbl9zdGFrZQAAAAAAAAAAAAABAAAACw==",
        "AAAAAAAAAVZSZWdpc3RyYXRpb24gaXMganVkZ2VkIG9uICoqZnJlZSoqIHN0YWtlLCBiZWNhdXNlIHRoZSB0aGluZyBhbiBhdWRpdG9yCm5lZWRzIGJlZm9yZSB2b3VjaGluZyBmb3Igb25lIG1vcmUgY2VydGlmaWNhdGUgaXMgY2FwaXRhbCB0aGF0IGlzIG5vdAphbHJlYWR5IHZvdWNoaW5nIGZvciBhbm90aGVyIG9uZS4gVW5kZXIgdGhlIG9sZCBnbG9iYWwtc3Rha2UgbW9kZWwgdGhpcwpyZWFkIHRoZSB3aG9sZSBib29rLCBzbyBhIHNpbmdsZSAkNTAwIHN0YWtlIGNvdWxkIGJhY2sgYW4gdW5saW1pdGVkCm51bWJlciBvZiBjZXJ0aWZpY2F0ZXMgYXQgJDUwMCBvZiBhZHZlcnRpc2VkIGNvbGxhdGVyYWwgZWFjaC4AAAAAAA1pc19yZWdpc3RlcmVkAAAAAAAAAQAAAAAAAAAHYXVkaXRvcgAAAAATAAAAAQAAAAE=",
        "AAAAAAAAAAAAAAAOZ2V0X2FsbG9jYXRpb24AAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAL",
        "AAAAAAAAAHdDYXBpdGFsIG5vdCBjdXJyZW50bHkgc3RhbmRpbmcgYmVoaW5kIGFueSBjZXJ0aWZpY2F0ZS4gVGhpcyBpcyB3aGF0IGNhbgpiZSBhbGxvY2F0ZWQgdG8gYSBuZXcgYXR0ZXN0YXRpb24gb3Igd2l0aGRyYXduLgAAAAAOZ2V0X2ZyZWVfc3Rha2UAAAAAAAEAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAEAAAAL",
        "AAAAAAAAAf9TbGFzaCBhIGNlcnRpZmljYXRlJ3MgYWxsb2NhdGlvbiB0byB0aGUgKip0cmVhc3VyeSoqLCBhbmQgbm93aGVyZSBlbHNlLgoKVGhlIHJlY2lwaWVudCBpcyBhIHBhcmFtZXRlciBvbmx5IHNvIHRoZSBDaGFsbGVuZ2VNYW5hZ2VyIGNhbiBwYXNzIHRoZQp0cmVhc3VyeSBhZGRyZXNzIGl0IHdhcyBpbml0aWFsaXplZCB3aXRoOyB0aGVyZSBpcyBkZWxpYmVyYXRlbHkgbm8gcGF0aAp0aGF0IGxldHMgYSBjaGFsbGVuZ2VyLCBhIHZpY3RpbSBvciBhbiBvcGVyYXRvciBuYW1lIHRoZW1zZWx2ZXMgaGVyZS4KU2xhc2hlZCBzdGFrZSBtdXN0IG5ldmVyIGJlIGEgcHJpemUsIG9yIG1hbnVmYWN0dXJpbmcgYSB0cnVlIHByb29mCmJlY29tZXMgYSBidXNpbmVzcyBtb2RlbC4KClRoZSBkcmF3IGlzIGNhcHBlZCBieSAqdGhpcyBjZXJ0aWZpY2F0ZSdzKiBhbGxvY2F0aW9uLCBzbyBzbGFzaGluZwpjZXJ0aWZpY2F0ZSBBIGNhbm5vdCB0b3VjaCB0aGUgYWxsb2NhdGlvbiBiYWNraW5nIGNlcnRpZmljYXRlIEIuAAAAABBzbGFzaF9hbGxvY2F0aW9uAAAAAwAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAh0cmVhc3VyeQAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAA=",
        "AAAAAAAAAR5SZXRpcmUgYW4gYWxsb2NhdGlvbiBhdCBzZXR0bGVtZW50OiB3aGF0ZXZlciB3YXMgbm90IHNsYXNoZWQgZ29lcyBiYWNrCnRvIHRoZSBhdWRpdG9yJ3MgKipmcmVlKiogc3Rha2UuCgpXaXRob3V0IHRoaXMgdGhlIHVuc2xhc2hlZCByZW1haW5kZXIgd291bGQgc2l0IGFsbG9jYXRlZCB0byBhIGRlYWQKY2VydGlmaWNhdGUgZm9yZXZlciDigJQgY2FwaXRhbCBzdHJhbmRlZCwgd2hpY2ggaXMgdGhlIGV4YWN0IGRlZmVjdCB0aGUKcGVyLWNlcnRpZmljYXRlIHJlZmFjdG9yIGV4aXN0cyB0byByZW1vdmUuAAAAAAARcmV0aXJlX2FsbG9jYXRpb24AAAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAAA",
        "AAAAAAAAAIxUaGUgYXVkaXRvciBmcmVlcyBhIGxpdmUgYWxsb2NhdGlvbiB0aGVtc2VsdmVzLCBvbmNlIHRoZSBjZXJ0aWZpY2F0ZSdzCnNldHRsZW1lbnQgZGVhZGxpbmUgaGFzIHBhc3NlZCBhbmQgbm8gY2hhbGxlbmdlIGNhbiBzdGlsbCBsYW5kIG9uIGl0LgAAABJyZWxlYXNlX2FsbG9jYXRpb24AAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAA=",
        "AAAAAAAAAAAAAAAUYWxsb2NhdGlvbl91bmxvY2tfYXQAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAABg==",
        "AAAAAAAAAAAAAAAWZ2V0X2FsbG9jYXRpb25fYXVkaXRvcgAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAA+gAAAAT" ]),
      options
    )
  }
  public readonly fromJSON = {
    stake: this.txFromJSON<null>,
        release: this.txFromJSON<null>,
        allocate: this.txFromJSON<null>,
        get_stake: this.txFromJSON<i128>,
        initialize: this.txFromJSON<null>,
        locked_until: this.txFromJSON<u64>,
        get_allocated: this.txFromJSON<i128>,
        get_min_stake: this.txFromJSON<i128>,
        is_registered: this.txFromJSON<boolean>,
        get_allocation: this.txFromJSON<i128>,
        get_free_stake: this.txFromJSON<i128>,
        slash_allocation: this.txFromJSON<null>,
        retire_allocation: this.txFromJSON<null>,
        release_allocation: this.txFromJSON<null>,
        allocation_unlock_at: this.txFromJSON<u64>,
        get_allocation_auditor: this.txFromJSON<Option<string>>
  }
}