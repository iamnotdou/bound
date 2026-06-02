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
    contractId: "CDN6S5DKUCC4O33L3RGTTO4LYNJVPLPIYYZTUANPJJZYAHZCR32O4WFB",
  }
} as const

export type DataKey = {tag: "Operator", values: void} | {tag: "ChallengeManager", values: void} | {tag: "Token", values: void} | {tag: "Balance", values: void} | {tag: "Locked", values: void} | {tag: "UnlockAt", values: void};

export interface Client {
  /**
   * Construct and simulate a deposit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  deposit: ({amount}: {amount: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({operator, challenge_manager, token, unlock_at}: {operator: string, challenge_manager: string, token: string, unlock_at: u64}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_balance transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_balance: (options?: AssembledTransactionOptions<i128>) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a release_to_victim transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  release_to_victim: ({victim, amount}: {victim: string, amount: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a release_to_operator transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  release_to_operator: (options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

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
      new ContractSpec([ "AAAAAAAAAAAAAAAHZGVwb3NpdAAAAAABAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAA",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAABgAAAAAAAAAAAAAACE9wZXJhdG9yAAAAAAAAAAAAAAAQQ2hhbGxlbmdlTWFuYWdlcgAAAAAAAAAAAAAABVRva2VuAAAAAAAAAAAAAAAAAAAHQmFsYW5jZQAAAAAAAAAAAAAAAAZMb2NrZWQAAAAAAAAAAAAAAAAACFVubG9ja0F0",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAABAAAAAAAAAAIb3BlcmF0b3IAAAATAAAAAAAAABFjaGFsbGVuZ2VfbWFuYWdlcgAAAAAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAJdW5sb2NrX2F0AAAAAAAABgAAAAA=",
        "AAAAAAAAAAAAAAALZ2V0X2JhbGFuY2UAAAAAAAAAAAEAAAAL",
        "AAAAAAAAAAAAAAARcmVsZWFzZV90b192aWN0aW0AAAAAAAACAAAAAAAAAAZ2aWN0aW0AAAAAABMAAAAAAAAABmFtb3VudAAAAAAACwAAAAA=",
        "AAAAAAAAAAAAAAATcmVsZWFzZV90b19vcGVyYXRvcgAAAAAAAAAAAA==" ]),
      options
    )
  }
  public readonly fromJSON = {
    deposit: this.txFromJSON<null>,
        initialize: this.txFromJSON<null>,
        get_balance: this.txFromJSON<i128>,
        release_to_victim: this.txFromJSON<null>,
        release_to_operator: this.txFromJSON<null>
  }
}