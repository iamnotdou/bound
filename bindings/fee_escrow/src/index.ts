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
    contractId: "CD4EZ5FCFC7D65OHBB5HHF6ASM4OCRGNAO6XQZQ5LKH4QB327SF5EYOF",
  }
} as const

export type DataKey = {tag: "ChallengeManager", values: void} | {tag: "Token", values: void} | {tag: "Operator", values: void} | {tag: "Auditor", values: void} | {tag: "Amount", values: void} | {tag: "Released", values: void};

export interface Client {
  /**
   * Construct and simulate a deposit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  deposit: ({operator, auditor, amount}: {operator: string, auditor: string, amount: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_amount transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_amount: (options?: AssembledTransactionOptions<i128>) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({challenge_manager, token}: {challenge_manager: string, token: string}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a is_released transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  is_released: (options?: AssembledTransactionOptions<boolean>) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a release_to_auditor transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  release_to_auditor: (options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a slash_to_challenger transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  slash_to_challenger: ({challenger}: {challenger: string}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

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
      new ContractSpec([ "AAAAAAAAAAAAAAAHZGVwb3NpdAAAAAADAAAAAAAAAAhvcGVyYXRvcgAAABMAAAAAAAAAB2F1ZGl0b3IAAAAAEwAAAAAAAAAGYW1vdW50AAAAAAALAAAAAA==",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAABgAAAAAAAAAAAAAAEENoYWxsZW5nZU1hbmFnZXIAAAAAAAAAAAAAAAVUb2tlbgAAAAAAAAAAAAAAAAAACE9wZXJhdG9yAAAAAAAAAAAAAAAHQXVkaXRvcgAAAAAAAAAAAAAAAAZBbW91bnQAAAAAAAAAAAAAAAAACFJlbGVhc2Vk",
        "AAAAAAAAAAAAAAAKZ2V0X2Ftb3VudAAAAAAAAAAAAAEAAAAL",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAAAgAAAAAAAAARY2hhbGxlbmdlX21hbmFnZXIAAAAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAA",
        "AAAAAAAAAAAAAAALaXNfcmVsZWFzZWQAAAAAAAAAAAEAAAAB",
        "AAAAAAAAAAAAAAAScmVsZWFzZV90b19hdWRpdG9yAAAAAAAAAAAAAA==",
        "AAAAAAAAAAAAAAATc2xhc2hfdG9fY2hhbGxlbmdlcgAAAAABAAAAAAAAAApjaGFsbGVuZ2VyAAAAAAATAAAAAA==" ]),
      options
    )
  }
  public readonly fromJSON = {
    deposit: this.txFromJSON<null>,
        get_amount: this.txFromJSON<i128>,
        initialize: this.txFromJSON<null>,
        is_released: this.txFromJSON<boolean>,
        release_to_auditor: this.txFromJSON<null>,
        slash_to_challenger: this.txFromJSON<null>
  }
}