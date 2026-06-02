import { Buffer } from "buffer";
import { Address } from '@stellar/stellar-sdk';
import {
  AssembledTransaction,
  Client as ContractClient,
  ClientOptions as ContractClientOptions,
  MethodOptions,
  Result,
  Spec as ContractSpec,
} from '@stellar/stellar-sdk/contract';
import type {
  u32,
  i32,
  u64,
  i64,
  u128,
  i128,
  u256,
  i256,
  AssembledTransactionOptions,
  Option,
  Typepoint,
  Duration,
} from '@stellar/stellar-sdk/contract';
export * from '@stellar/stellar-sdk'
export * as contract from '@stellar/stellar-sdk/contract'
export * as rpc from '@stellar/stellar-sdk/rpc'

if (typeof window !== 'undefined') {
  //@ts-ignore Buffer exists
  window.Buffer = window.Buffer || Buffer;
}


export const networks = {
  testnet: {
    networkPassphrase: "Test SDF Network ; September 2015",
    contractId: "CCSJTEXOJZ322XI5ZJF6YZ2IRLCRKNXTGHGK3ZLATKL6Y7CGQODS4VZB",
  }
} as const

export type DataKey = {tag: "ChallengeManager", values: void} | {tag: "Registry", values: void} | {tag: "Token", values: void} | {tag: "MinRegistrationStake", values: void} | {tag: "Stake", values: readonly [string]} | {tag: "LockedUntil", values: readonly [string]};

export interface Client {
  /**
   * Construct and simulate a lock transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  lock: ({auditor, until}: {auditor: string, until: u64}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a slash transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  slash: ({auditor, recipient, amount}: {auditor: string, recipient: string, amount: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  stake: ({auditor, amount}: {auditor: string, amount: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a release transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  release: ({auditor}: {auditor: string}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_stake: ({auditor}: {auditor: string}, options?: AssembledTransactionOptions<i128>) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({challenge_manager, registry, token, min_stake}: {challenge_manager: string, registry: string, token: string, min_stake: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a locked_until transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  locked_until: ({auditor}: {auditor: string}, options?: AssembledTransactionOptions<u64>) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_min_stake transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_min_stake: (options?: AssembledTransactionOptions<i128>) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a is_registered transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  is_registered: ({auditor}: {auditor: string}, options?: AssembledTransactionOptions<boolean>) => Promise<AssembledTransaction<boolean>>

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
      new ContractSpec([ "AAAAAAAAAAAAAAAEbG9jawAAAAIAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAAAAAAFdW50aWwAAAAAAAAGAAAAAA==",
        "AAAAAAAAAAAAAAAFc2xhc2gAAAAAAAADAAAAAAAAAAdhdWRpdG9yAAAAABMAAAAAAAAACXJlY2lwaWVudAAAAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAA=",
        "AAAAAAAAAAAAAAAFc3Rha2UAAAAAAAACAAAAAAAAAAdhdWRpdG9yAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAA=",
        "AAAAAAAAAAAAAAAHcmVsZWFzZQAAAAABAAAAAAAAAAdhdWRpdG9yAAAAABMAAAAA",
        "AAAAAAAAAAAAAAAJZ2V0X3N0YWtlAAAAAAAAAQAAAAAAAAAHYXVkaXRvcgAAAAATAAAAAQAAAAs=",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAABgAAAAAAAAAAAAAAEENoYWxsZW5nZU1hbmFnZXIAAAAAAAAAAAAAAAhSZWdpc3RyeQAAAAAAAAAAAAAABVRva2VuAAAAAAAAAAAAAAAAAAAUTWluUmVnaXN0cmF0aW9uU3Rha2UAAAABAAAAAAAAAAVTdGFrZQAAAAAAAAEAAAATAAAAAQAAAAAAAAALTG9ja2VkVW50aWwAAAAAAQAAABM=",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAABAAAAAAAAAARY2hhbGxlbmdlX21hbmFnZXIAAAAAAAATAAAAAAAAAAhyZWdpc3RyeQAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAJbWluX3N0YWtlAAAAAAAACwAAAAA=",
        "AAAAAAAAAAAAAAAMbG9ja2VkX3VudGlsAAAAAQAAAAAAAAAHYXVkaXRvcgAAAAATAAAAAQAAAAY=",
        "AAAAAAAAAAAAAAANZ2V0X21pbl9zdGFrZQAAAAAAAAAAAAABAAAACw==",
        "AAAAAAAAAAAAAAANaXNfcmVnaXN0ZXJlZAAAAAAAAAEAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAEAAAAB" ]),
      options
    )
  }
  public readonly fromJSON = {
    lock: this.txFromJSON<null>,
        slash: this.txFromJSON<null>,
        stake: this.txFromJSON<null>,
        release: this.txFromJSON<null>,
        get_stake: this.txFromJSON<i128>,
        initialize: this.txFromJSON<null>,
        locked_until: this.txFromJSON<u64>,
        get_min_stake: this.txFromJSON<i128>,
        is_registered: this.txFromJSON<boolean>
  }
}