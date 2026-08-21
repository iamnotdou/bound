# Deliverables

The three things this repo is being judged on, stated verbatim, plus an
audit of what is actually built. Nothing here is aspiration: every line
marked BUILT was checked against source, not against a design document.

## R1 — Close the proof gap in the ChallengeManager

> Build a PaymentRouter contract that agent payments flow through, recording
> cumulative spend per certificate — making `BoundExceeded` provable purely
> from on-chain state (tracked spend > certified bound). The router is
> rail-agnostic by design: the demo's existing x402-style payment flow
> (HTTP 402 → pay → retry) will settle through it, and the standard x402
> facilitator integration planned for Instaward 2 plugs into this same
> spend-tracking hook rather than replacing it. Add trustless verification
> for `ExpiredCertificate` (ledger time vs. expiry). Both join the existing
> `InsufficientReserve` proof, shrinking the arbiter to an explicitly-labeled
> edge case. Full unit + integration tests for every proof path (fraud and
> no-fraud branches), with CI (GitHub Actions build + test on every PR).
>
> The protocol's whole promise is "if the guarantee is a lie, anyone can
> prove it." Today that's only true for one lie. This deliverable makes the
> challenger's case verifiable by code, not by a referee.

## R2 — Give the insurance layer an economy

> Operators pay an ongoing coverage premium priced on bound × duration when a
> certificate is published; premiums accrue to the auditor as yield on their
> staked capital, with a configurable protocol fee share captured by the
> system. Extend the FeeEscrow / add a PremiumVault contract: premium deposit,
> time-based accrual, auditor claim, fee split — and the slashing interaction
> (a slashed auditor forfeits unclaimed yield). Tested and deployed to
> testnet, surfaced in the SDK and demo UI.
>
> Turns auditing from a demo role into a business: stake capital, earn
> premiums, lose everything if you lie. This is the protocol's revenue model —
> the question every Build Award reviewer will ask — answered with working
> code instead of a slide.

## R3 — Package it for adoption

> Extract the SDK from the demo app into an independently installable, typed
> npm package (`@bound/sdk`) covering the full lifecycle including the new
> flows (routed payments, all three fraud proofs, premiums); package the MCP
> connector so any MCP-capable agent gets Bound's tools out of the box; ship a
> reproducible quickstart, a permanently hosted demo running the full
> lifecycle on testnet (issue → pay through the router → challenge →
> automatic payout → auditor yield), and a recorded walkthrough video.
>
> Converts code into adoption — a minutes-not-weeks path for any agent
> framework — and gives the Ambassador clear, clickable, non-technical
> evidence that the whole loop works on-chain.

---

# Status

## R1

| Requirement                                                      | State   |
| ---------------------------------------------------------------- | ------- |
| PaymentRouter contract, cumulative spend per certificate         | BUILT   |
| `BoundExceeded` provable from on-chain state alone               | BUILT   |
| `ExpiredCertificate` trustless (ledger time vs. expiry)          | BUILT   |
| Arbiter shrunk to a labelled edge case (`FakeSignature` only)    | BUILT   |
| Unit + integration tests, fraud and no-fraud branch, every proof | BUILT   |
| GitHub Actions build + test on every PR                          | BUILT   |
| **Agent payments actually flow through the router**              | **GAP** |

The router is deployed and the ChallengeManager reads it, but nothing writes
to it. `BoundClient.executePayment` calls the raw USDC SAC, and `enroll` is
never called outside contract tests. Spend is therefore never recorded, so
`BoundExceeded` cannot fire on anything the demo does. The mechanism is
correct and tested; the feed is missing.

## R2

| Requirement                                                         | State   |
| ------------------------------------------------------------------- | ------- |
| PremiumVault: deposit, time-based accrual, auditor claim, fee split | BUILT   |
| Slashed auditor forfeits unclaimed yield                            | BUILT   |
| Tested                                                              | BUILT   |
| Deployed to testnet                                                 | BUILT   |
| Surfaced in the SDK                                                 | **GAP** |
| Surfaced in the demo UI                                             | **GAP** |

`BoundClient` exposes no premium method at all. `premiumVault` appears in the
SDK only as an address in `config.ts` and `deployments.ts`.

## R3

| Requirement                                              | State                      |
| -------------------------------------------------------- | -------------------------- |
| `@bound/sdk` independently installable, typed, published | BUILT (0.4.0)              |
| Covers routed payments                                   | **GAP**                    |
| Covers all three fraud proofs                            | PARTIAL                    |
| Covers premiums                                          | **GAP**                    |
| MCP connector packaged for any MCP-capable agent         | BUILT (`@bound/mcp` 0.1.0) |
| Reproducible quickstart                                  | BUILT                      |
| Permanently hosted demo                                  | BUILT                      |
| Hosted demo runs the _full_ lifecycle                    | **GAP**                    |
| Recorded walkthrough video                               | NOT STARTED                |

---

# What is left

**The hosted demo's settlement leg, and the video.** Both are blocked on the
same thing, and it is a real protocol property rather than missing work: the
claim window is 72 hours. `pnpm run demo` proves everything up to and including
a claim being admitted from on-chain state, and settles the _false_ claim in
its own filing transaction. `pnpm run demo:settle` closes the window and pays
out three days later. Collapsing the window so a demo finishes in one run would
be demonstrating a protocol we deliberately do not ship — v1 settled the first
claim to arrive and foreclosed every honest one behind it, and the window is
what fixed that.

So the honest sequence is: run the demo, wait out the window on a certificate
already filed, then record the walkthrough against a certificate whose whole
lifecycle is finished and permanently readable on-chain.
