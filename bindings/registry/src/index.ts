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
    contractId: "CA5MERYJFQS4UTBYJM644K2YWWTXABWNTYQC6GUD4S2ZE2DHBVUPUJAJ",
  }
} as const

export type DataKey = {tag: "ChallengeManager", values: void} | {tag: "AuditorStaking", values: void} | {tag: "CertCount", values: void} | {tag: "Certificate", values: readonly [u64]} | {tag: "AgentCert", values: readonly [string]};

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
   */
  attest: ({auditor, cert_id}: {auditor: string, cert_id: u64}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a verify transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  verify: ({agent}: {agent: string}, options?: AssembledTransactionOptions<VerifyResult>) => Promise<AssembledTransaction<VerifyResult>>

  /**
   * Construct and simulate a publish transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  publish: ({operator, agent, bound, reserve_amount, expires_at, reserve_vault_contract, auditor_staking_contract}: {operator: string, agent: string, bound: i128, reserve_amount: i128, expires_at: u64, reserve_vault_contract: string, auditor_staking_contract: string}, options?: AssembledTransactionOptions<u64>) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  initialize: ({challenge_manager, auditor_staking}: {challenge_manager: string, auditor_staking: string}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a invalidate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  invalidate: ({cert_id}: {cert_id: u64}, options?: AssembledTransactionOptions<null>) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_cert_id transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_id: ({agent}: {agent: string}, options?: AssembledTransactionOptions<u64>) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_cert_count transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_count: (options?: AssembledTransactionOptions<u64>) => Promise<AssembledTransaction<u64>>

  /**
   * Construct and simulate a get_certificate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_certificate: ({cert_id}: {cert_id: u64}, options?: AssembledTransactionOptions<Certificate>) => Promise<AssembledTransaction<Certificate>>

  /**
   * Construct and simulate a get_cert_auditor transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_auditor: ({cert_id}: {cert_id: u64}, options?: AssembledTransactionOptions<string>) => Promise<AssembledTransaction<string>>

  /**
   * Construct and simulate a get_cert_reserve transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_cert_reserve: ({cert_id}: {cert_id: u64}, options?: AssembledTransactionOptions<i128>) => Promise<AssembledTransaction<i128>>

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
      new ContractSpec([ "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" ]),
      options
    )
  }
  public readonly fromJSON = {
    attest: this.txFromJSON<null>,
        verify: this.txFromJSON<VerifyResult>,
        publish: this.txFromJSON<u64>,
        initialize: this.txFromJSON<null>,
        invalidate: this.txFromJSON<null>,
        get_cert_id: this.txFromJSON<u64>,
        get_cert_count: this.txFromJSON<u64>,
        get_certificate: this.txFromJSON<Certificate>,
        get_cert_auditor: this.txFromJSON<string>,
        get_cert_reserve: this.txFromJSON<i128>
  }
}