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

Audited against source, not against design docs, and — where it was possible to
run the thing — against live testnet rather than against a passing test.

## R1

| Requirement                                                      | State                  |
| ---------------------------------------------------------------- | ---------------------- |
| PaymentRouter contract, cumulative spend per certificate         | BUILT                  |
| `BoundExceeded` provable from on-chain state alone               | BUILT                  |
| `ExpiredCertificate` trustless (ledger time vs. expiry)          | BUILT                  |
| Arbiter shrunk to a labelled edge case (`FakeSignature` only)    | BUILT                  |
| Unit + integration tests, fraud and no-fraud branch, every proof | BUILT (201 Rust tests) |
| GitHub Actions build + test on every PR                          | BUILT                  |
| Agent payments actually flow through the router                  | BUILT                  |

That last row was the one real gap in this deliverable, and it was a bad one:
the router was deployed and the ChallengeManager read its counter, but nothing
wrote to it. `executePayment` called the raw USDC asset contract and `enroll`
was never called outside the contract tests, so the number `BoundExceeded` is
proven from was always zero. The mechanism was correct and tested; the feed was
missing.

`executePayment` now routes an enrolled signer through the router and returns a
receipt saying which rail it took; `enrollAgent` is what puts an address on
that rail. **Verified live:** certificate #5 on testnet carries $600 of routed
spend against a $500 bound, and a `BoundExceeded` claim was admitted from that
state alone.

## R2

| Requirement                                                         | State |
| ------------------------------------------------------------------- | ----- |
| PremiumVault: deposit, time-based accrual, auditor claim, fee split | BUILT |
| Slashed auditor forfeits unclaimed yield                            | BUILT |
| Tested                                                              | BUILT |
| Deployed to testnet                                                 | BUILT |
| Surfaced in the SDK                                                 | BUILT |
| Surfaced in the demo UI                                             | BUILT |

`quotePremium`, `quotePremiumForCert`, `payPremium`, `premiumAccrued`,
`premiumClaimable`, `premiumPaid`, `coverage`, `claimPremium`. Read live off
certificate #5: premium 11,402 stroops, accruing at ~3.17 stroops/second in a
straight line across the term.

Panels on both the dashboard and the public certificate page. A quote is
rendered as a quote and never as an accrual of zero — the two are different
claims and only one of them is ever true.

## R3

| Requirement                                              | State                                |
| -------------------------------------------------------- | ------------------------------------ |
| `@bound/sdk` independently installable, typed, published | BUILT (0.5.0, provenance-signed)     |
| Covers routed payments                                   | BUILT                                |
| Covers all three fraud proofs                            | BUILT                                |
| Covers premiums                                          | BUILT                                |
| MCP connector packaged for any MCP-capable agent         | BUILT (`@bound/mcp` 0.1.1, 15 tools) |
| Reproducible quickstart                                  | BUILT                                |
| Permanently hosted demo                                  | BUILT (www.boundprotocol.dev/app)    |
| Hosted demo shows routed spend and the coverage economy  | BUILT                                |
| Hosted demo runs the _full_ lifecycle                    | PARTIAL — see below                  |
| Recorded walkthrough video                               | NOT DOING — by decision              |

---

# What is left, and why

**The settlement leg of the lifecycle.** It waits on a real protocol property
rather than on missing work: the claim window is 72 hours.

`pnpm run demo` drives everything up to and including a claim being admitted
from on-chain state, and it _does_ settle one claim live — the false one, which
is rejected and settled inside its own filing transaction with the bond forfeit,
because a predicate the contract can evaluate itself needs no window when it
comes out false. The true claim opens a window. `pnpm run demo:settle` closes it
and pays out three days later.

Collapsing that window so a demo finishes in one run would mean demonstrating a
protocol we deliberately do not ship. v1 settled the first claim to arrive and
foreclosed every honest one behind it; the window is what fixed that, and a
claim filed in hour 71 is admitted on the same terms as one filed in hour 1.

So the sequence is: run the demo, let the window on an already-filed certificate
lapse, then settle it. What that leaves behind is better evidence than a
recording anyway — a certificate whose entire lifecycle is finished and
permanently readable on-chain by anyone who wants to check it.

**The walkthrough video is a deliberate omission.** The deliverable asks for one
and there will not be one. The hosted demo is clickable, the quickstart is
reproducible from a clean clone, and the settled certificate is public. A
reviewer who wants to see the loop can run it or read it rather than watch
somebody else run it.

**One release step needs a human.** npm Trusted Publishing can only be
configured on a package that already exists, so `@bound/mcp`'s first publish has
to be done by hand before the release tag can take it over. The v0.5.0 tag
published `@bound/sdk` and then failed on exactly this, as expected.
