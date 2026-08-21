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
    contractId: "CD6VK2YUUZO5L5R76DNT3NQNWR2UCALPQ3T6STWAQSEY7Q6I5P4LAFC3",
  }
} as const

export type DataKey = {tag: "Registry", values: void} | {tag: "ChallengeManager", values: void} | {tag: "Token", values: void} | {tag: "Balance", values: readonly [u64]} | {tag: "Locked", values: readonly [u64]} | {tag: "UnlockAt", values: readonly [u64]};

export interface Client {
  /**
   * Construct and simulate a deposit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  deposit: ({cert_id, amount}: {cert_id: u64, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a is_locked transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  is_locked: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({registry, challenge_manager, token}: {registry: string, challenge_manager: string, token: string}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_balance transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_balance: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_unlock_at transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_unlock_at: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a pay_from_reserve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Only the ChallengeManager can call this — it pays out of the reserve of
   * the certificate under challenge, and no other.
   * 
   * Both settlement payments run through here: the victim's compensation and
   * the challenger's fee. Both are drawn from **the operator's own reserve
   * for this certificate**, which is what makes self-dealing a wash — a
   * colluding operator paying its own colluding "victim" is moving money
   * from its left pocket to its right.
   */
  pay_from_reserve: ({cert_id, to, amount}: {cert_id: u64, to: string, amount: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a release_to_operator transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Operator reclaims this certificate's reserve only after its challenge
   * window has closed — `expires_at + CHALLENGE_WINDOW`, not `expires_at`.
   */
  release_to_operator: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

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
      new ContractSpec([ "AAAAAAAAAAAAAAAHZGVwb3NpdAAAAAACAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAAAAAAABmFtb3VudAAAAAAACwAAAAA=",
        "AAAAAAAAAAAAAAAJaXNfbG9ja2VkAAAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAE=",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAABgAAAAAAAAAAAAAACFJlZ2lzdHJ5AAAAAAAAAAAAAAAQQ2hhbGxlbmdlTWFuYWdlcgAAAAAAAAAAAAAABVRva2VuAAAAAAAAAQAAAAAAAAAHQmFsYW5jZQAAAAABAAAABgAAAAEAAAAAAAAABkxvY2tlZAAAAAAAAQAAAAYAAAABAAABakxlZGdlciB0aW1lIGF0IHdoaWNoIHRoaXMgY2VydGlmaWNhdGUncyByZXNlcnZlIG1heSBiZSByZWNsYWltZWQgYnkgaXRzCm9wZXJhdG9yOiB0aGUgY2VydGlmaWNhdGUncyAqc2V0dGxlbWVudCBkZWFkbGluZSoKKGBleHBpcmVzX2F0ICsgQ0hBTExFTkdFX1dJTkRPV2ApLCBub3QgaXRzIGV4cGlyeS4gQSBwcm9vZiBhYm91dApwb3N0LWV4cGlyeSBhY3Rpdml0eSBvbmx5IGJlY29tZXMgcHJvdmFibGUgYWZ0ZXIgZXhwaXJ5OyB1bmxvY2tpbmcgYXQKYGV4cGlyZXNfYXRgIHdvdWxkIGxldCB0aGUgb3BlcmF0b3Igd2l0aGRyYXcgdGhlIHJlc2VydmUgYmVmb3JlIHRoZQpwcm9vZiBjb3VsZCBldmVyIGJlIGZpbGVkIGFnYWluc3QgaXQuAAAAAAAIVW5sb2NrQXQAAAABAAAABg==",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAAAwAAAAAAAAAIcmVnaXN0cnkAAAATAAAAAAAAABFjaGFsbGVuZ2VfbWFuYWdlcgAAAAAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAA=",
        "AAAAAAAAAAAAAAALZ2V0X2JhbGFuY2UAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAs=",
        "AAAAAAAAAAAAAAANZ2V0X3VubG9ja19hdAAAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAG",
        "AAAAAAAAAbdPbmx5IHRoZSBDaGFsbGVuZ2VNYW5hZ2VyIGNhbiBjYWxsIHRoaXMg4oCUIGl0IHBheXMgb3V0IG9mIHRoZSByZXNlcnZlIG9mCnRoZSBjZXJ0aWZpY2F0ZSB1bmRlciBjaGFsbGVuZ2UsIGFuZCBubyBvdGhlci4KCkJvdGggc2V0dGxlbWVudCBwYXltZW50cyBydW4gdGhyb3VnaCBoZXJlOiB0aGUgdmljdGltJ3MgY29tcGVuc2F0aW9uIGFuZAp0aGUgY2hhbGxlbmdlcidzIGZlZS4gQm90aCBhcmUgZHJhd24gZnJvbSAqKnRoZSBvcGVyYXRvcidzIG93biByZXNlcnZlCmZvciB0aGlzIGNlcnRpZmljYXRlKiosIHdoaWNoIGlzIHdoYXQgbWFrZXMgc2VsZi1kZWFsaW5nIGEgd2FzaCDigJQgYQpjb2xsdWRpbmcgb3BlcmF0b3IgcGF5aW5nIGl0cyBvd24gY29sbHVkaW5nICJ2aWN0aW0iIGlzIG1vdmluZyBtb25leQpmcm9tIGl0cyBsZWZ0IHBvY2tldCB0byBpdHMgcmlnaHQuAAAAABBwYXlfZnJvbV9yZXNlcnZlAAAAAwAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAJ0bwAAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAA==",
        "AAAAAAAAAI5PcGVyYXRvciByZWNsYWltcyB0aGlzIGNlcnRpZmljYXRlJ3MgcmVzZXJ2ZSBvbmx5IGFmdGVyIGl0cyBjaGFsbGVuZ2UKd2luZG93IGhhcyBjbG9zZWQg4oCUIGBleHBpcmVzX2F0ICsgQ0hBTExFTkdFX1dJTkRPV2AsIG5vdCBgZXhwaXJlc19hdGAuAAAAAAATcmVsZWFzZV90b19vcGVyYXRvcgAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAAA" ]),
      options
    )
  }
  public readonly fromJSON = {
    deposit: this.txFromJSON<null>,
        is_locked: this.txFromJSON<boolean>,
        initialize: this.txFromJSON<null>,
        get_balance: this.txFromJSON<i128>,
        get_unlock_at: this.txFromJSON<u64>,
        pay_from_reserve: this.txFromJSON<null>,
        release_to_operator: this.txFromJSON<null>
  }
}