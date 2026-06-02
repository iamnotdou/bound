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
    contractId: "CD6LZRYQQKVC44QIVFZALUNRGFYCJ7WWSFVU47ATR6TM6UHKSKONB4XW",
  }
} as const

export type DataKey = {tag: "Registry", values: void} | {tag: "AuditorStaking", values: void} | {tag: "ReserveVault", values: void} | {tag: "FeeEscrow", values: void} | {tag: "Token", values: void} | {tag: "Arbiter", values: void} | {tag: "MinStake", values: void} | {tag: "Challenge", values: readonly [u64]} | {tag: "ChallengeCount", values: void};

export type Verdict = {tag: "Pending", values: void} | {tag: "ChallengeWins", values: void} | {tag: "ChallengeFails", values: void};


export interface Challenge {
  cert_id: u64;
  challenger: string;
  proof_type: ProofType;
  stake: i128;
  verdict: Verdict;
  victim: string;
}

export type ProofType = {tag: "InsufficientReserve", values: void} | {tag: "BoundExceeded", values: void} | {tag: "FakeSignature", values: void} | {tag: "ExpiredCertificate", values: void};

export interface Client {
  /**
   * Construct and simulate a resolve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  resolve: ({challenge_id}: {challenge_id: u64}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a challenge transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  challenge: ({challenger, cert_id, proof_type, victim, stake}: {challenger: string, cert_id: u64, proof_type: ProofType, victim: string, stake: i128}, options?: AssembledTransactionOptions<u64>) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({registry, auditor_staking, reserve_vault, fee_escrow, token, arbiter, min_stake}: {registry: string, auditor_staking: string, reserve_vault: string, fee_escrow: string, token: string, arbiter: string, min_stake: i128}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_challenge transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_challenge: ({challenge_id}: {challenge_id: u64}, options?: AssembledTransactionOptions<Challenge>) => Promise<AssembledTransaction<Challenge>>

  /**
   * Construct and simulate a resolve_by_arbiter transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  resolve_by_arbiter: ({challenge_id, fraud_proven}: {challenge_id: u64, fraud_proven: boolean}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_challenge_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_challenge_count: (options?: AssembledTransactionOptions<u64>) => Promise<AssembledTransaction<u64>>

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
      new ContractSpec([ "AAAAAAAAAAAAAAAHcmVzb2x2ZQAAAAABAAAAAAAAAAxjaGFsbGVuZ2VfaWQAAAAGAAAAAA==",
        "AAAAAAAAAAAAAAAJY2hhbGxlbmdlAAAAAAAABQAAAAAAAAAKY2hhbGxlbmdlcgAAAAAAEwAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAApwcm9vZl90eXBlAAAAAAfQAAAACVByb29mVHlwZQAAAAAAAAAAAAAGdmljdGltAAAAAAATAAAAAAAAAAVzdGFrZQAAAAAAAAsAAAABAAAABg==",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAACQAAAAAAAAAAAAAACFJlZ2lzdHJ5AAAAAAAAAAAAAAAOQXVkaXRvclN0YWtpbmcAAAAAAAAAAAAAAAAADFJlc2VydmVWYXVsdAAAAAAAAAAAAAAACUZlZUVzY3JvdwAAAAAAAAAAAAAAAAAABVRva2VuAAAAAAAAAAAAAAAAAAAHQXJiaXRlcgAAAAAAAAAAAAAAAAhNaW5TdGFrZQAAAAEAAAAAAAAACUNoYWxsZW5nZQAAAAAAAAEAAAAGAAAAAAAAAAAAAAAOQ2hhbGxlbmdlQ291bnQAAA==",
        "AAAAAgAAAAAAAAAAAAAAB1ZlcmRpY3QAAAAAAwAAAAAAAAAAAAAAB1BlbmRpbmcAAAAAAAAAAAAAAAANQ2hhbGxlbmdlV2lucwAAAAAAAAAAAAAAAAAADkNoYWxsZW5nZUZhaWxzAAA=",
        "AAAAAAAAAAAAAAAKaW5pdGlhbGl6ZQAAAAAABwAAAAAAAAAIcmVnaXN0cnkAAAATAAAAAAAAAA9hdWRpdG9yX3N0YWtpbmcAAAAAEwAAAAAAAAANcmVzZXJ2ZV92YXVsdAAAAAAAABMAAAAAAAAACmZlZV9lc2Nyb3cAAAAAABMAAAAAAAAABXRva2VuAAAAAAAAEwAAAAAAAAAHYXJiaXRlcgAAAAATAAAAAAAAAAltaW5fc3Rha2UAAAAAAAALAAAAAA==",
        "AAAAAQAAAAAAAAAAAAAACUNoYWxsZW5nZQAAAAAAAAYAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAAAAAAKY2hhbGxlbmdlcgAAAAAAEwAAAAAAAAAKcHJvb2ZfdHlwZQAAAAAH0AAAAAlQcm9vZlR5cGUAAAAAAAAAAAAABXN0YWtlAAAAAAAACwAAAAAAAAAHdmVyZGljdAAAAAfQAAAAB1ZlcmRpY3QAAAAAAAAAAAZ2aWN0aW0AAAAAABM=",
        "AAAAAgAAAAAAAAAAAAAACVByb29mVHlwZQAAAAAAAAQAAAAAAAAAAAAAABNJbnN1ZmZpY2llbnRSZXNlcnZlAAAAAAAAAAAAAAAADUJvdW5kRXhjZWVkZWQAAAAAAAAAAAAAAAAAAA1GYWtlU2lnbmF0dXJlAAAAAAAAAAAAAAAAAAASRXhwaXJlZENlcnRpZmljYXRlAAA=",
        "AAAAAAAAAAAAAAANZ2V0X2NoYWxsZW5nZQAAAAAAAAEAAAAAAAAADGNoYWxsZW5nZV9pZAAAAAYAAAABAAAH0AAAAAlDaGFsbGVuZ2UAAAA=",
        "AAAAAAAAAAAAAAAScmVzb2x2ZV9ieV9hcmJpdGVyAAAAAAACAAAAAAAAAAxjaGFsbGVuZ2VfaWQAAAAGAAAAAAAAAAxmcmF1ZF9wcm92ZW4AAAABAAAAAA==",
        "AAAAAAAAAAAAAAATZ2V0X2NoYWxsZW5nZV9jb3VudAAAAAAAAAAAAQAAAAY=" ]),
      options
    )
  }
  public readonly fromJSON = {
    resolve: this.txFromJSON<null>,
        challenge: this.txFromJSON<u64>,
        initialize: this.txFromJSON<null>,
        get_challenge: this.txFromJSON<Challenge>,
        resolve_by_arbiter: this.txFromJSON<null>,
        get_challenge_count: this.txFromJSON<u64>
  }
}