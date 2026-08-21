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
    contractId: "CA5JT2IBPY7X4QZS65XY6YW2BXDUEFPZHXTNG2OCPCJTOM4DWEWEOHAF",
  }
} as const

export type DataKey = {tag: "Registry", values: void} | {tag: "ChallengeManager", values: void} | {tag: "Token", values: void} | {tag: "Treasury", values: void} | {tag: "RateBps", values: void} | {tag: "FeeBps", values: void} | {tag: "Coverage", values: readonly [u64]};


/**
 * One certificate's coverage: what the operator paid, what the auditor has
 * earned so far, and what is left to earn.
 * 
 * Stored under `DataKey::Coverage(cert_id)` — the per-certificate storage style
 * the ReserveVault established. One vault contract serves many certificates and
 * each certificate's money is walled off from every other one: paying for cert
 * A can never fund a claim on cert B, and forfeiting A can never touch B.
 */
export interface Coverage {
  /**
 * The auditor entitled to the yield, snapshotted at payment time for the
 * same reason.
 */
auditor: string;
  /**
 * How much of `yield_pot` the auditor has already withdrawn. Theirs
 * permanently — see `forfeit`.
 */
claimed: i128;
  /**
 * Set when the coverage is closed — by a slash (`forfeit`) or by a hygiene
 * kill (`terminate`). Closing rewrites `yield_pot` to the exact amount
 * frozen in place and stops accrual dead: from then on `accrued` returns
 * `yield_pot` outright and the clock is irrelevant.
 */
closed: boolean;
  /**
 * The ledger instant the coverage was closed at. A record, not an input to
 * the accrual arithmetic — see `closed`.
 */
closed_at: u64;
  /**
 * Coverage length in seconds: `expires_at - issued_at`.
 */
duration: u64;
  /**
 * The operator who paid. Snapshotted, so a later Registry change cannot
 * re-point who this coverage belonged to.
 */
payer: string;
  /**
 * Total the operator paid, in stroops.
 */
premium: i128;
  /**
 * The protocol's share, already transferred to the treasury at payment
 * time. Recorded so `premium == protocol_fee + yield_pot` is auditable
 * from storage alone.
 */
protocol_fee: i128;
  /**
 * Coverage start. The certificate's `issued_at`, not the payment time.
 */
start: u64;
  /**
 * `premium - protocol_fee`: the most the auditor can ever receive from
 * this certificate.
 */
yield_pot: i128;
}

export interface Client {
  /**
   * Construct and simulate a claim transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The auditor withdraws accrued-and-unclaimed yield.
   * 
   * **Claiming is allowed at any time, including mid-coverage.** Straight-line
   * accrual makes that the natural reading — at every instant the accrued
   * figure is precisely payment for coverage already delivered, and making the
   * auditor wait until expiry would be an interest-free loan from the auditor
   * to the protocol for no security gain.
   * 
   * It is safe *because* of the forfeiture rule, not in spite of it. Forfeiture
   * only ever takes **unclaimed** yield, so an auditor who claims continuously
   * is converting forfeitable yield into settled income as fast as they earn
   * it. That is deliberate and it is honestly priced: the auditor's skin in
   * the game is their **allocation**, which stays fully slashable however fast
   * they claim. The premium is yield on that capital, not a second bond, and
   * pretending an unclaimed premium is collateral would over-state the
   * protocol's teeth.
   * 
   * The cost, stated plainly: a diligent auditor who claims every block
   * forfeits almost nothing on a slash. The
   */
  claim: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a quote transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The price of covering `bound` for `duration_seconds`.
   * 
   * ```text
   * premium = bound * rate_bps * duration_seconds / (10_000 * SECONDS_PER_YEAR)
   * ```
   * 
   * **Truncation.** Integer division truncates toward zero, and the remainder
   * is deliberately dropped rather than rounded. That direction is chosen,
   * not accidental: the operator is charged **no more** than the exact price,
   * which keeps the quote a hard ceiling a counterparty can verify by hand.
   * The same truncation is applied to the fee share and to accrual, where it
   * runs the other way — the auditor accrues **no more** than the exact
   * figure — so the vault can never owe out more than it holds. Every one of
   * these errors is at most one stroop (10⁻⁷ USDC).
   * 
   * Concretely, a $1,500 bound at 200 bps for 90 days is
   * `15_000_000_000 * 200 * 7_776_000 / (10_000 * 31_536_000)`
   * `= 73_972_602` stroops (exactly $7.3972602 before truncation of the
   * trailing `.7397…`).
   * 
   * **Overflow.** `overflow-checks = true` is on for release, but this uses
   * explicit `checked_mul` so a hostile `bound`
   */
  quote: ({bound, duration_seconds}: {bound: i128, duration_seconds: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a accrued transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * Total yield accrued to this certificate's auditor so far, capped at
   * `yield_pot` and never falling.
   * 
   * Straight-line: `yield_pot * elapsed / duration`. Before `start` it is
   * zero; at or past `start + duration` it is the whole pot; after the
   * coverage is closed it is frozen at the closing instant. There is no path
   * by which this exceeds `yield_pot`, including arbitrarily far past expiry.
   */
  accrued: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a forfeit transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * **Step 4 of the settlement waterfall.** The auditor was slashed for this
   * certificate, so they forfeit **unclaimed** yield on it.
   * 
   * The split, and the reason for each half:
   * 
   * - **Accrued-but-unclaimed → the victim**, capped by `victim_cap` (the
   * harm the operator's own reserve did not already cover). This is
   * consistent with the waterfall's first invariant — *victim compensation
   * comes only from the operator's own money* — because **the premium is
   * the operator's money**. Every stroop in this pot was paid in by the
   * operator of this certificate and has not yet been handed to anybody
   * else. Compensating a victim from it is the same left-pocket-to-right
   * move that makes self-dealing a wash on the reserve.
   * - **Everything above the cap, and the entire unaccrued remainder → the
   * treasury.** The cap is what preserves the second invariant, *everything
   * stays capped by proven harm*: the victim can never receive more than
   * the harm that was proven, no matter how large the premium is. The
   * unaccrued remainder is nobody's —
   */
  forfeit: ({cert_id, victim, victim_cap}: {cert_id: u64, victim: string, victim_cap: i128}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a is_paid transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  is_paid: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>

  /**
   * Construct and simulate a claimable transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * What the auditor could withdraw right now: accrued minus already claimed.
   */
  claimable: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a terminate transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * **Hygiene mode.** The certificate was killed on a true proof that
   * evidences no harm, so nobody was hurt and the auditor was not slashed.
   * 
   * The premium is handled the only way consistent with that: accrual stops
   * at the kill, the auditor **keeps** what accrued up to it and can still
   * claim it, and the unaccrued remainder goes to the treasury.
   * 
   * - The auditor keeps the accrued share because hygiene mode explicitly
   * does not blame them — they covered the period they covered.
   * - The unaccrued remainder is not refunded to the operator, for the same
   * reason DESIGN-V2 §10 already gives: a dead certificate is supposed to
   * cost the operator their certificate, their reserve lockup **and their
   * premium**. Refunding it would make killing your own certificate free,
   * which is exactly the manufacturable breach hygiene mode exists to price.
   */
  terminate: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a initialize transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * `rate_bps` and `fee_bps` are **simple, transparent, configurable
   * parameters**, deliberately.
   * 
   * There is no actuarial model here, no risk tiering and no external
   * underwriter. The premium is `bound × rate × duration`, annualised, and
   * nothing else. Risk-based pricing would need a loss history this protocol
   * does not have, and inventing one in code would be a lie dressed as a
   * model. When there is real loss data, `rate_bps` is the single number that
   * changes — at a fresh deployment, since there is no admin.
   */
  initialize: ({registry, challenge_manager, token, treasury, rate_bps, fee_bps}: {registry: string, challenge_manager: string, token: string, treasury: string, rate_bps: i128, fee_bps: i128}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a quote_cert transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The price this certificate would pay, read from its own `bound` and its
   * own coverage window. Callable before payment, so an operator can see the
   * number before committing to it.
   */
  quote_cert: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_claimed transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_claimed: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_fee_bps transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_fee_bps: (options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_premium transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_premium: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a pay_premium transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   * The operator pays this certificate's coverage premium. Once, ever.
   * 
   * **Which duration.** `expires_at - issued_at`, both immutable fields of
   * the certificate — *not* `expires_at - now`. Two reasons, and the second
   * is the load-bearing one:
   * 
   * 1. Coverage runs from the moment the certificate exists, so that is the
   * period being priced.
   * 2. Pricing from `now` would make the premium a function of **when the
   * operator chooses to call this**. An operator would simply wait, and a
   * year's coverage would be bought for a day's price the instant before
   * expiry. Anchoring to `issued_at` removes the choice entirely: the
   * price is fixed at publish and no transaction timing can move it.
   * 
   * The certificate must be attested — the premium is yield on **staked**
   * capital, so there has to be an auditor with capital allocated to it
   * before there is anything to accrue to.
   * 
   * A zero-duration or zero-bound certificate prices at zero and is recorded
   * as paid without moving a stroop. It does not panic: the Registry already
   * rejects both at `publish`
   */
  pay_premium: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<null>>

  /**
   * Construct and simulate a get_coverage transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_coverage: ({cert_id}: {cert_id: u64}, options?: MethodOptions) => Promise<AssembledTransaction<Coverage>>

  /**
   * Construct and simulate a get_rate_bps transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_rate_bps: (options?: MethodOptions) => Promise<AssembledTransaction<i128>>

  /**
   * Construct and simulate a get_treasury transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
   */
  get_treasury: (options?: MethodOptions) => Promise<AssembledTransaction<string>>

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
      new ContractSpec([ "AAAAAAAABABUaGUgYXVkaXRvciB3aXRoZHJhd3MgYWNjcnVlZC1hbmQtdW5jbGFpbWVkIHlpZWxkLgoKKipDbGFpbWluZyBpcyBhbGxvd2VkIGF0IGFueSB0aW1lLCBpbmNsdWRpbmcgbWlkLWNvdmVyYWdlLioqIFN0cmFpZ2h0LWxpbmUKYWNjcnVhbCBtYWtlcyB0aGF0IHRoZSBuYXR1cmFsIHJlYWRpbmcg4oCUIGF0IGV2ZXJ5IGluc3RhbnQgdGhlIGFjY3J1ZWQKZmlndXJlIGlzIHByZWNpc2VseSBwYXltZW50IGZvciBjb3ZlcmFnZSBhbHJlYWR5IGRlbGl2ZXJlZCwgYW5kIG1ha2luZyB0aGUKYXVkaXRvciB3YWl0IHVudGlsIGV4cGlyeSB3b3VsZCBiZSBhbiBpbnRlcmVzdC1mcmVlIGxvYW4gZnJvbSB0aGUgYXVkaXRvcgp0byB0aGUgcHJvdG9jb2wgZm9yIG5vIHNlY3VyaXR5IGdhaW4uCgpJdCBpcyBzYWZlICpiZWNhdXNlKiBvZiB0aGUgZm9yZmVpdHVyZSBydWxlLCBub3QgaW4gc3BpdGUgb2YgaXQuIEZvcmZlaXR1cmUKb25seSBldmVyIHRha2VzICoqdW5jbGFpbWVkKiogeWllbGQsIHNvIGFuIGF1ZGl0b3Igd2hvIGNsYWltcyBjb250aW51b3VzbHkKaXMgY29udmVydGluZyBmb3JmZWl0YWJsZSB5aWVsZCBpbnRvIHNldHRsZWQgaW5jb21lIGFzIGZhc3QgYXMgdGhleSBlYXJuCml0LiBUaGF0IGlzIGRlbGliZXJhdGUgYW5kIGl0IGlzIGhvbmVzdGx5IHByaWNlZDogdGhlIGF1ZGl0b3IncyBza2luIGluCnRoZSBnYW1lIGlzIHRoZWlyICoqYWxsb2NhdGlvbioqLCB3aGljaCBzdGF5cyBmdWxseSBzbGFzaGFibGUgaG93ZXZlciBmYXN0CnRoZXkgY2xhaW0uIFRoZSBwcmVtaXVtIGlzIHlpZWxkIG9uIHRoYXQgY2FwaXRhbCwgbm90IGEgc2Vjb25kIGJvbmQsIGFuZApwcmV0ZW5kaW5nIGFuIHVuY2xhaW1lZCBwcmVtaXVtIGlzIGNvbGxhdGVyYWwgd291bGQgb3Zlci1zdGF0ZSB0aGUKcHJvdG9jb2wncyB0ZWV0aC4KClRoZSBjb3N0LCBzdGF0ZWQgcGxhaW5seTogYSBkaWxpZ2VudCBhdWRpdG9yIHdobyBjbGFpbXMgZXZlcnkgYmxvY2sKZm9yZmVpdHMgYWxtb3N0IG5vdGhpbmcgb24gYSBzbGFzaC4gVGhlAAAABWNsYWltAAAAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAs=",
        "AAAAAAAABABUaGUgcHJpY2Ugb2YgY292ZXJpbmcgYGJvdW5kYCBmb3IgYGR1cmF0aW9uX3NlY29uZHNgLgoKYGBgdGV4dApwcmVtaXVtID0gYm91bmQgKiByYXRlX2JwcyAqIGR1cmF0aW9uX3NlY29uZHMgLyAoMTBfMDAwICogU0VDT05EU19QRVJfWUVBUikKYGBgCgoqKlRydW5jYXRpb24uKiogSW50ZWdlciBkaXZpc2lvbiB0cnVuY2F0ZXMgdG93YXJkIHplcm8sIGFuZCB0aGUgcmVtYWluZGVyCmlzIGRlbGliZXJhdGVseSBkcm9wcGVkIHJhdGhlciB0aGFuIHJvdW5kZWQuIFRoYXQgZGlyZWN0aW9uIGlzIGNob3NlbiwKbm90IGFjY2lkZW50YWw6IHRoZSBvcGVyYXRvciBpcyBjaGFyZ2VkICoqbm8gbW9yZSoqIHRoYW4gdGhlIGV4YWN0IHByaWNlLAp3aGljaCBrZWVwcyB0aGUgcXVvdGUgYSBoYXJkIGNlaWxpbmcgYSBjb3VudGVycGFydHkgY2FuIHZlcmlmeSBieSBoYW5kLgpUaGUgc2FtZSB0cnVuY2F0aW9uIGlzIGFwcGxpZWQgdG8gdGhlIGZlZSBzaGFyZSBhbmQgdG8gYWNjcnVhbCwgd2hlcmUgaXQKcnVucyB0aGUgb3RoZXIgd2F5IOKAlCB0aGUgYXVkaXRvciBhY2NydWVzICoqbm8gbW9yZSoqIHRoYW4gdGhlIGV4YWN0CmZpZ3VyZSDigJQgc28gdGhlIHZhdWx0IGNhbiBuZXZlciBvd2Ugb3V0IG1vcmUgdGhhbiBpdCBob2xkcy4gRXZlcnkgb25lIG9mCnRoZXNlIGVycm9ycyBpcyBhdCBtb3N0IG9uZSBzdHJvb3AgKDEw4oG74oG3IFVTREMpLgoKQ29uY3JldGVseSwgYSAkMSw1MDAgYm91bmQgYXQgMjAwIGJwcyBmb3IgOTAgZGF5cyBpcwpgMTVfMDAwXzAwMF8wMDAgKiAyMDAgKiA3Xzc3Nl8wMDAgLyAoMTBfMDAwICogMzFfNTM2XzAwMClgCmA9IDczXzk3Ml82MDJgIHN0cm9vcHMgKGV4YWN0bHkgJDcuMzk3MjYwMiBiZWZvcmUgdHJ1bmNhdGlvbiBvZiB0aGUKdHJhaWxpbmcgYC43Mzk34oCmYCkuCgoqKk92ZXJmbG93LioqIGBvdmVyZmxvdy1jaGVja3MgPSB0cnVlYCBpcyBvbiBmb3IgcmVsZWFzZSwgYnV0IHRoaXMgdXNlcwpleHBsaWNpdCBgY2hlY2tlZF9tdWxgIHNvIGEgaG9zdGlsZSBgYm91bmRgAAAABXF1b3RlAAAAAAAAAgAAAAAAAAAFYm91bmQAAAAAAAALAAAAAAAAABBkdXJhdGlvbl9zZWNvbmRzAAAABgAAAAEAAAAL",
        "AAAAAAAAAX9Ub3RhbCB5aWVsZCBhY2NydWVkIHRvIHRoaXMgY2VydGlmaWNhdGUncyBhdWRpdG9yIHNvIGZhciwgY2FwcGVkIGF0CmB5aWVsZF9wb3RgIGFuZCBuZXZlciBmYWxsaW5nLgoKU3RyYWlnaHQtbGluZTogYHlpZWxkX3BvdCAqIGVsYXBzZWQgLyBkdXJhdGlvbmAuIEJlZm9yZSBgc3RhcnRgIGl0IGlzCnplcm87IGF0IG9yIHBhc3QgYHN0YXJ0ICsgZHVyYXRpb25gIGl0IGlzIHRoZSB3aG9sZSBwb3Q7IGFmdGVyIHRoZQpjb3ZlcmFnZSBpcyBjbG9zZWQgaXQgaXMgZnJvemVuIGF0IHRoZSBjbG9zaW5nIGluc3RhbnQuIFRoZXJlIGlzIG5vIHBhdGgKYnkgd2hpY2ggdGhpcyBleGNlZWRzIGB5aWVsZF9wb3RgLCBpbmNsdWRpbmcgYXJiaXRyYXJpbHkgZmFyIHBhc3QgZXhwaXJ5LgAAAAAHYWNjcnVlZAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAACw==",
        "AAAAAAAABAAqKlN0ZXAgNCBvZiB0aGUgc2V0dGxlbWVudCB3YXRlcmZhbGwuKiogVGhlIGF1ZGl0b3Igd2FzIHNsYXNoZWQgZm9yIHRoaXMKY2VydGlmaWNhdGUsIHNvIHRoZXkgZm9yZmVpdCAqKnVuY2xhaW1lZCoqIHlpZWxkIG9uIGl0LgoKVGhlIHNwbGl0LCBhbmQgdGhlIHJlYXNvbiBmb3IgZWFjaCBoYWxmOgoKLSAqKkFjY3J1ZWQtYnV0LXVuY2xhaW1lZCDihpIgdGhlIHZpY3RpbSoqLCBjYXBwZWQgYnkgYHZpY3RpbV9jYXBgICh0aGUKaGFybSB0aGUgb3BlcmF0b3IncyBvd24gcmVzZXJ2ZSBkaWQgbm90IGFscmVhZHkgY292ZXIpLiBUaGlzIGlzCmNvbnNpc3RlbnQgd2l0aCB0aGUgd2F0ZXJmYWxsJ3MgZmlyc3QgaW52YXJpYW50IOKAlCAqdmljdGltIGNvbXBlbnNhdGlvbgpjb21lcyBvbmx5IGZyb20gdGhlIG9wZXJhdG9yJ3Mgb3duIG1vbmV5KiDigJQgYmVjYXVzZSAqKnRoZSBwcmVtaXVtIGlzCnRoZSBvcGVyYXRvcidzIG1vbmV5KiouIEV2ZXJ5IHN0cm9vcCBpbiB0aGlzIHBvdCB3YXMgcGFpZCBpbiBieSB0aGUKb3BlcmF0b3Igb2YgdGhpcyBjZXJ0aWZpY2F0ZSBhbmQgaGFzIG5vdCB5ZXQgYmVlbiBoYW5kZWQgdG8gYW55Ym9keQplbHNlLiBDb21wZW5zYXRpbmcgYSB2aWN0aW0gZnJvbSBpdCBpcyB0aGUgc2FtZSBsZWZ0LXBvY2tldC10by1yaWdodAptb3ZlIHRoYXQgbWFrZXMgc2VsZi1kZWFsaW5nIGEgd2FzaCBvbiB0aGUgcmVzZXJ2ZS4KLSAqKkV2ZXJ5dGhpbmcgYWJvdmUgdGhlIGNhcCwgYW5kIHRoZSBlbnRpcmUgdW5hY2NydWVkIHJlbWFpbmRlciDihpIgdGhlCnRyZWFzdXJ5LioqIFRoZSBjYXAgaXMgd2hhdCBwcmVzZXJ2ZXMgdGhlIHNlY29uZCBpbnZhcmlhbnQsICpldmVyeXRoaW5nCnN0YXlzIGNhcHBlZCBieSBwcm92ZW4gaGFybSo6IHRoZSB2aWN0aW0gY2FuIG5ldmVyIHJlY2VpdmUgbW9yZSB0aGFuCnRoZSBoYXJtIHRoYXQgd2FzIHByb3Zlbiwgbm8gbWF0dGVyIGhvdyBsYXJnZSB0aGUgcHJlbWl1bSBpcy4gVGhlCnVuYWNjcnVlZCByZW1haW5kZXIgaXMgbm9ib2R5J3Mg4oCUAAAAB2ZvcmZlaXQAAAAAAwAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAAAAAAZ2aWN0aW0AAAAAABMAAAAAAAAACnZpY3RpbV9jYXAAAAAAAAsAAAABAAAACw==",
        "AAAAAAAAAAAAAAAHaXNfcGFpZAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAAAQ==",
        "AAAAAAAAAElXaGF0IHRoZSBhdWRpdG9yIGNvdWxkIHdpdGhkcmF3IHJpZ2h0IG5vdzogYWNjcnVlZCBtaW51cyBhbHJlYWR5IGNsYWltZWQuAAAAAAAACWNsYWltYWJsZQAAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAL",
        "AAAAAAAAAz0qKkh5Z2llbmUgbW9kZS4qKiBUaGUgY2VydGlmaWNhdGUgd2FzIGtpbGxlZCBvbiBhIHRydWUgcHJvb2YgdGhhdApldmlkZW5jZXMgbm8gaGFybSwgc28gbm9ib2R5IHdhcyBodXJ0IGFuZCB0aGUgYXVkaXRvciB3YXMgbm90IHNsYXNoZWQuCgpUaGUgcHJlbWl1bSBpcyBoYW5kbGVkIHRoZSBvbmx5IHdheSBjb25zaXN0ZW50IHdpdGggdGhhdDogYWNjcnVhbCBzdG9wcwphdCB0aGUga2lsbCwgdGhlIGF1ZGl0b3IgKiprZWVwcyoqIHdoYXQgYWNjcnVlZCB1cCB0byBpdCBhbmQgY2FuIHN0aWxsCmNsYWltIGl0LCBhbmQgdGhlIHVuYWNjcnVlZCByZW1haW5kZXIgZ29lcyB0byB0aGUgdHJlYXN1cnkuCgotIFRoZSBhdWRpdG9yIGtlZXBzIHRoZSBhY2NydWVkIHNoYXJlIGJlY2F1c2UgaHlnaWVuZSBtb2RlIGV4cGxpY2l0bHkKZG9lcyBub3QgYmxhbWUgdGhlbSDigJQgdGhleSBjb3ZlcmVkIHRoZSBwZXJpb2QgdGhleSBjb3ZlcmVkLgotIFRoZSB1bmFjY3J1ZWQgcmVtYWluZGVyIGlzIG5vdCByZWZ1bmRlZCB0byB0aGUgb3BlcmF0b3IsIGZvciB0aGUgc2FtZQpyZWFzb24gREVTSUdOLVYyIMKnMTAgYWxyZWFkeSBnaXZlczogYSBkZWFkIGNlcnRpZmljYXRlIGlzIHN1cHBvc2VkIHRvCmNvc3QgdGhlIG9wZXJhdG9yIHRoZWlyIGNlcnRpZmljYXRlLCB0aGVpciByZXNlcnZlIGxvY2t1cCAqKmFuZCB0aGVpcgpwcmVtaXVtKiouIFJlZnVuZGluZyBpdCB3b3VsZCBtYWtlIGtpbGxpbmcgeW91ciBvd24gY2VydGlmaWNhdGUgZnJlZSwKd2hpY2ggaXMgZXhhY3RseSB0aGUgbWFudWZhY3R1cmFibGUgYnJlYWNoIGh5Z2llbmUgbW9kZSBleGlzdHMgdG8gcHJpY2UuAAAAAAAACXRlcm1pbmF0ZQAAAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAEAAAAL",
        "AAAAAgAAAAAAAAAAAAAAB0RhdGFLZXkAAAAABwAAAAAAAAAAAAAACFJlZ2lzdHJ5AAAAAAAAAAAAAAAQQ2hhbGxlbmdlTWFuYWdlcgAAAAAAAAAAAAAABVRva2VuAAAAAAAAAAAAAQ5XaGVyZSB0aGUgcHJvdG9jb2wgZmVlIHNoYXJlIGFuZCBldmVyeSBmb3JmZWl0ZWQgc3Ryb29wIGdvLiBOYW1lZCBvbmNlCmF0IGBpbml0aWFsaXplYCwgd2l0aCBubyBhZG1pbiBhbmQgbm8gc2V0dGVyIOKAlCB0aGUgc2FtZSBydWxlIHRoZQpDaGFsbGVuZ2VNYW5hZ2VyJ3MgdHJlYXN1cnkgZm9sbG93cywgYW5kIGZvciB0aGUgc2FtZSByZWFzb246IGEgbXV0YWJsZQpkZXN0aW5hdGlvbiBmb3IgZm9yZmVpdGVkIG1vbmV5IGlzIGEgcHJpemUgc29tZWJvZHkgY2FuIGFpbS4AAAAAAAhUcmVhc3VyeQAAAAAAAAA7VGhlIGFubnVhbGlzZWQgY292ZXJhZ2UgcmF0ZSwgaW4gYmFzaXMgcG9pbnRzIG9mIHRoZSBib3VuZC4AAAAAB1JhdGVCcHMAAAAAAAAAAEVUaGUgcHJvdG9jb2wncyBzaGFyZSBvZiBlYWNoIHByZW1pdW0sIGluIGJhc2lzIHBvaW50cyBvZiB0aGUgcHJlbWl1bS4AAAAAAAAGRmVlQnBzAAAAAAABAAAAAAAAAAhDb3ZlcmFnZQAAAAEAAAAG",
        "AAAAAAAAAfxgcmF0ZV9icHNgIGFuZCBgZmVlX2Jwc2AgYXJlICoqc2ltcGxlLCB0cmFuc3BhcmVudCwgY29uZmlndXJhYmxlCnBhcmFtZXRlcnMqKiwgZGVsaWJlcmF0ZWx5LgoKVGhlcmUgaXMgbm8gYWN0dWFyaWFsIG1vZGVsIGhlcmUsIG5vIHJpc2sgdGllcmluZyBhbmQgbm8gZXh0ZXJuYWwKdW5kZXJ3cml0ZXIuIFRoZSBwcmVtaXVtIGlzIGBib3VuZCDDlyByYXRlIMOXIGR1cmF0aW9uYCwgYW5udWFsaXNlZCwgYW5kCm5vdGhpbmcgZWxzZS4gUmlzay1iYXNlZCBwcmljaW5nIHdvdWxkIG5lZWQgYSBsb3NzIGhpc3RvcnkgdGhpcyBwcm90b2NvbApkb2VzIG5vdCBoYXZlLCBhbmQgaW52ZW50aW5nIG9uZSBpbiBjb2RlIHdvdWxkIGJlIGEgbGllIGRyZXNzZWQgYXMgYQptb2RlbC4gV2hlbiB0aGVyZSBpcyByZWFsIGxvc3MgZGF0YSwgYHJhdGVfYnBzYCBpcyB0aGUgc2luZ2xlIG51bWJlciB0aGF0CmNoYW5nZXMg4oCUIGF0IGEgZnJlc2ggZGVwbG95bWVudCwgc2luY2UgdGhlcmUgaXMgbm8gYWRtaW4uAAAACmluaXRpYWxpemUAAAAAAAYAAAAAAAAACHJlZ2lzdHJ5AAAAEwAAAAAAAAARY2hhbGxlbmdlX21hbmFnZXIAAAAAAAATAAAAAAAAAAV0b2tlbgAAAAAAABMAAAAAAAAACHRyZWFzdXJ5AAAAEwAAAAAAAAAIcmF0ZV9icHMAAAALAAAAAAAAAAdmZWVfYnBzAAAAAAsAAAAA",
        "AAAAAAAAALBUaGUgcHJpY2UgdGhpcyBjZXJ0aWZpY2F0ZSB3b3VsZCBwYXksIHJlYWQgZnJvbSBpdHMgb3duIGBib3VuZGAgYW5kIGl0cwpvd24gY292ZXJhZ2Ugd2luZG93LiBDYWxsYWJsZSBiZWZvcmUgcGF5bWVudCwgc28gYW4gb3BlcmF0b3IgY2FuIHNlZSB0aGUKbnVtYmVyIGJlZm9yZSBjb21taXR0aW5nIHRvIGl0LgAAAApxdW90ZV9jZXJ0AAAAAAABAAAAAAAAAAdjZXJ0X2lkAAAAAAYAAAABAAAACw==",
        "AAAAAQAAAaVPbmUgY2VydGlmaWNhdGUncyBjb3ZlcmFnZTogd2hhdCB0aGUgb3BlcmF0b3IgcGFpZCwgd2hhdCB0aGUgYXVkaXRvciBoYXMKZWFybmVkIHNvIGZhciwgYW5kIHdoYXQgaXMgbGVmdCB0byBlYXJuLgoKU3RvcmVkIHVuZGVyIGBEYXRhS2V5OjpDb3ZlcmFnZShjZXJ0X2lkKWAg4oCUIHRoZSBwZXItY2VydGlmaWNhdGUgc3RvcmFnZSBzdHlsZQp0aGUgUmVzZXJ2ZVZhdWx0IGVzdGFibGlzaGVkLiBPbmUgdmF1bHQgY29udHJhY3Qgc2VydmVzIG1hbnkgY2VydGlmaWNhdGVzIGFuZAplYWNoIGNlcnRpZmljYXRlJ3MgbW9uZXkgaXMgd2FsbGVkIG9mZiBmcm9tIGV2ZXJ5IG90aGVyIG9uZTogcGF5aW5nIGZvciBjZXJ0CkEgY2FuIG5ldmVyIGZ1bmQgYSBjbGFpbSBvbiBjZXJ0IEIsIGFuZCBmb3JmZWl0aW5nIEEgY2FuIG5ldmVyIHRvdWNoIEIuAAAAAAAAAAAAAAhDb3ZlcmFnZQAAAAoAAABTVGhlIGF1ZGl0b3IgZW50aXRsZWQgdG8gdGhlIHlpZWxkLCBzbmFwc2hvdHRlZCBhdCBwYXltZW50IHRpbWUgZm9yIHRoZQpzYW1lIHJlYXNvbi4AAAAAB2F1ZGl0b3IAAAAAEwAAAGBIb3cgbXVjaCBvZiBgeWllbGRfcG90YCB0aGUgYXVkaXRvciBoYXMgYWxyZWFkeSB3aXRoZHJhd24uIFRoZWlycwpwZXJtYW5lbnRseSDigJQgc2VlIGBmb3JmZWl0YC4AAAAHY2xhaW1lZAAAAAALAAABCFNldCB3aGVuIHRoZSBjb3ZlcmFnZSBpcyBjbG9zZWQg4oCUIGJ5IGEgc2xhc2ggKGBmb3JmZWl0YCkgb3IgYnkgYSBoeWdpZW5lCmtpbGwgKGB0ZXJtaW5hdGVgKS4gQ2xvc2luZyByZXdyaXRlcyBgeWllbGRfcG90YCB0byB0aGUgZXhhY3QgYW1vdW50CmZyb3plbiBpbiBwbGFjZSBhbmQgc3RvcHMgYWNjcnVhbCBkZWFkOiBmcm9tIHRoZW4gb24gYGFjY3J1ZWRgIHJldHVybnMKYHlpZWxkX3BvdGAgb3V0cmlnaHQgYW5kIHRoZSBjbG9jayBpcyBpcnJlbGV2YW50LgAAAAZjbG9zZWQAAAAAAAEAAABxVGhlIGxlZGdlciBpbnN0YW50IHRoZSBjb3ZlcmFnZSB3YXMgY2xvc2VkIGF0LiBBIHJlY29yZCwgbm90IGFuIGlucHV0IHRvCnRoZSBhY2NydWFsIGFyaXRobWV0aWMg4oCUIHNlZSBgY2xvc2VkYC4AAAAAAAAJY2xvc2VkX2F0AAAAAAAABgAAADVDb3ZlcmFnZSBsZW5ndGggaW4gc2Vjb25kczogYGV4cGlyZXNfYXQgLSBpc3N1ZWRfYXRgLgAAAAAAAAhkdXJhdGlvbgAAAAYAAABtVGhlIG9wZXJhdG9yIHdobyBwYWlkLiBTbmFwc2hvdHRlZCwgc28gYSBsYXRlciBSZWdpc3RyeSBjaGFuZ2UgY2Fubm90CnJlLXBvaW50IHdobyB0aGlzIGNvdmVyYWdlIGJlbG9uZ2VkIHRvLgAAAAAAAAVwYXllcgAAAAAAABMAAAAkVG90YWwgdGhlIG9wZXJhdG9yIHBhaWQsIGluIHN0cm9vcHMuAAAAB3ByZW1pdW0AAAAACwAAAJ1UaGUgcHJvdG9jb2wncyBzaGFyZSwgYWxyZWFkeSB0cmFuc2ZlcnJlZCB0byB0aGUgdHJlYXN1cnkgYXQgcGF5bWVudAp0aW1lLiBSZWNvcmRlZCBzbyBgcHJlbWl1bSA9PSBwcm90b2NvbF9mZWUgKyB5aWVsZF9wb3RgIGlzIGF1ZGl0YWJsZQpmcm9tIHN0b3JhZ2UgYWxvbmUuAAAAAAAADHByb3RvY29sX2ZlZQAAAAsAAABEQ292ZXJhZ2Ugc3RhcnQuIFRoZSBjZXJ0aWZpY2F0ZSdzIGBpc3N1ZWRfYXRgLCBub3QgdGhlIHBheW1lbnQgdGltZS4AAAAFc3RhcnQAAAAAAAAGAAAAVmBwcmVtaXVtIC0gcHJvdG9jb2xfZmVlYDogdGhlIG1vc3QgdGhlIGF1ZGl0b3IgY2FuIGV2ZXIgcmVjZWl2ZSBmcm9tCnRoaXMgY2VydGlmaWNhdGUuAAAAAAAJeWllbGRfcG90AAAAAAAACw==",
        "AAAAAAAAAAAAAAALZ2V0X2NsYWltZWQAAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAs=",
        "AAAAAAAAAAAAAAALZ2V0X2ZlZV9icHMAAAAAAAAAAAEAAAAL",
        "AAAAAAAAAAAAAAALZ2V0X3ByZW1pdW0AAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAAAs=",
        "AAAAAAAABABUaGUgb3BlcmF0b3IgcGF5cyB0aGlzIGNlcnRpZmljYXRlJ3MgY292ZXJhZ2UgcHJlbWl1bS4gT25jZSwgZXZlci4KCioqV2hpY2ggZHVyYXRpb24uKiogYGV4cGlyZXNfYXQgLSBpc3N1ZWRfYXRgLCBib3RoIGltbXV0YWJsZSBmaWVsZHMgb2YKdGhlIGNlcnRpZmljYXRlIOKAlCAqbm90KiBgZXhwaXJlc19hdCAtIG5vd2AuIFR3byByZWFzb25zLCBhbmQgdGhlIHNlY29uZAppcyB0aGUgbG9hZC1iZWFyaW5nIG9uZToKCjEuIENvdmVyYWdlIHJ1bnMgZnJvbSB0aGUgbW9tZW50IHRoZSBjZXJ0aWZpY2F0ZSBleGlzdHMsIHNvIHRoYXQgaXMgdGhlCnBlcmlvZCBiZWluZyBwcmljZWQuCjIuIFByaWNpbmcgZnJvbSBgbm93YCB3b3VsZCBtYWtlIHRoZSBwcmVtaXVtIGEgZnVuY3Rpb24gb2YgKip3aGVuIHRoZQpvcGVyYXRvciBjaG9vc2VzIHRvIGNhbGwgdGhpcyoqLiBBbiBvcGVyYXRvciB3b3VsZCBzaW1wbHkgd2FpdCwgYW5kIGEKeWVhcidzIGNvdmVyYWdlIHdvdWxkIGJlIGJvdWdodCBmb3IgYSBkYXkncyBwcmljZSB0aGUgaW5zdGFudCBiZWZvcmUKZXhwaXJ5LiBBbmNob3JpbmcgdG8gYGlzc3VlZF9hdGAgcmVtb3ZlcyB0aGUgY2hvaWNlIGVudGlyZWx5OiB0aGUKcHJpY2UgaXMgZml4ZWQgYXQgcHVibGlzaCBhbmQgbm8gdHJhbnNhY3Rpb24gdGltaW5nIGNhbiBtb3ZlIGl0LgoKVGhlIGNlcnRpZmljYXRlIG11c3QgYmUgYXR0ZXN0ZWQg4oCUIHRoZSBwcmVtaXVtIGlzIHlpZWxkIG9uICoqc3Rha2VkKioKY2FwaXRhbCwgc28gdGhlcmUgaGFzIHRvIGJlIGFuIGF1ZGl0b3Igd2l0aCBjYXBpdGFsIGFsbG9jYXRlZCB0byBpdApiZWZvcmUgdGhlcmUgaXMgYW55dGhpbmcgdG8gYWNjcnVlIHRvLgoKQSB6ZXJvLWR1cmF0aW9uIG9yIHplcm8tYm91bmQgY2VydGlmaWNhdGUgcHJpY2VzIGF0IHplcm8gYW5kIGlzIHJlY29yZGVkCmFzIHBhaWQgd2l0aG91dCBtb3ZpbmcgYSBzdHJvb3AuIEl0IGRvZXMgbm90IHBhbmljOiB0aGUgUmVnaXN0cnkgYWxyZWFkeQpyZWplY3RzIGJvdGggYXQgYHB1Ymxpc2hgAAAAC3BheV9wcmVtaXVtAAAAAAEAAAAAAAAAB2NlcnRfaWQAAAAABgAAAAA=",
        "AAAAAAAAAAAAAAAMZ2V0X2NvdmVyYWdlAAAAAQAAAAAAAAAHY2VydF9pZAAAAAAGAAAAAQAAB9AAAAAIQ292ZXJhZ2U=",
        "AAAAAAAAAAAAAAAMZ2V0X3JhdGVfYnBzAAAAAAAAAAEAAAAL",
        "AAAAAAAAAAAAAAAMZ2V0X3RyZWFzdXJ5AAAAAAAAAAEAAAAT" ]),
      options
    )
  }
  public readonly fromJSON = {
    claim: this.txFromJSON<i128>,
        quote: this.txFromJSON<i128>,
        accrued: this.txFromJSON<i128>,
        forfeit: this.txFromJSON<i128>,
        is_paid: this.txFromJSON<boolean>,
        claimable: this.txFromJSON<i128>,
        terminate: this.txFromJSON<i128>,
        initialize: this.txFromJSON<null>,
        quote_cert: this.txFromJSON<i128>,
        get_claimed: this.txFromJSON<i128>,
        get_fee_bps: this.txFromJSON<i128>,
        get_premium: this.txFromJSON<i128>,
        pay_premium: this.txFromJSON<null>,
        get_coverage: this.txFromJSON<Coverage>,
        get_rate_bps: this.txFromJSON<i128>,
        get_treasury: this.txFromJSON<string>
  }
}