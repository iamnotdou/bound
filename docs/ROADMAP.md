# Post-hackathon roadmap — toward SCF and InstAward

> Scale Track submission artifact. Bound did not start at this hackathon and does
> not end at it. This is what the integration built here unlocks, what it costs,
> and what would have to be true for it to be worth funding.

---

## 1. Where Bound already is

Not a prototype. The following is live and independently checkable:

|                                  |                                                                                                              |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| Seven Soroban contracts          | deployed to testnet, IDs in the [README](../README.md#deployed-contracts-stellar-testnet)                    |
| `@bound/sdk`                     | published to npm, typed client for the whole lifecycle                                                       |
| `@bound/mcp`                     | published to npm, 15 tools, works with any MCP-capable agent                                                 |
| Live app                         | [boundprotocol.dev/app](https://www.boundprotocol.dev/app)                                                   |
| Docs                             | [docs.boundprotocol.dev](https://docs.boundprotocol.dev)                                                     |
| Threat model + disclosed defects | published, including the ones still open                                                                     |
| InstAward                        | month 1 delivered, month 2 in flight                                                                         |
| A second deployment              | the same contracts denominated in the anchor's USDC, funded across a real SEP-24 deposit                     |
| A Turkish lira rail              | SEP-6 + SEP-38 against a TRY ⇄ USDC ramp, live at [bound-anchor.vercel.app](https://bound-anchor.vercel.app) |

The relevant point for a funder: **there is no "if we get funded we will build it" step
here.** The protocol exists, an award has already been delivered against it, and the
adversarial review that found the defects is public.

---

## 2. What this hackathon adds

A fiat boundary that reaches all the way to a bond, and the honest statement of
where it still stops.

**What runs.** SEP-1 discovery, SEP-10 with the anchor's challenge validated
before any wallet is asked to sign it, and transfers in both directions over
**whichever rail the anchor publishes** — SEP-24 where it hosts a form, SEP-6
where it does not, which is what the Turkish lira ramps are. The toml decides;
nothing is compiled in.

The second deployment is live. The same seven contracts, denominated in the
anchor's own USDC rather than a token the operator issues to itself, funded by a
completed SEP-24 deposit. On that deployment `/api/anchor` reports
`fundsReserve: true`: the money a deposit delivers is the money a reserve holds,
so fiat in funds a bond rather than stopping in a wallet. A full lifecycle —
publish, fund, attest, route payments, prove `BoundExceeded`, settle a false
claim — has run on it.

**What does not.** The TRY ramp we integrate is a sandbox: the bank is
simulated, KYC is auto-approved, and during this event its payout worker stalled,
leaving our 4 900 TRY deposit quoted at 99.94 USDC and sitting at
`pending_anchor`. The SEP surface it exposes is the one a real Turkish anchor
will expose, so that part of the integration is finished — but we have not yet
watched lira become a reserve end to end, and this document is not going to say
we have.

The distinction matters because the rail is the whole difference between a closed
on-chain demo and a product a business could be paid through. Everything Bound
does — bound a worst case, fund it, have an auditor stake on it, prove fraud by
arithmetic — was already true. Making it true about money anyone can spend is one
edge on a diagram and most of the remaining work.

---

## 3. The gap, named plainly

Two honest weaknesses. Naming them is cheaper than being caught with them.

**There is still no production TRY rail on Stellar.** Primary research, checked
directly against each provider: no anchor issues a TRY-backed asset; none accepts
TRY deposits or pays a Turkish IBAN; MoneyGram Ramps TR is cash-out only; MyKobo
has been down since 30 June 2026. A Turkish business cannot today receive money
through Stellar without leaving the region's currency.

What changed at this hackathon is our readiness for one, not the market. We
integrated a TRY ⇄ USDC ramp over the SEP surface a real Turkish anchor (BiLira
is the named candidate) would expose, so reaching production is a home domain and
a network passphrase rather than a diff. What we cannot claim is a live lira
counterparty: the one we built against simulates the bank, and the gap between
those two is exactly the thing an SDF anchor contact would close.

**Zero externally-committed payers.** The same research rated cross-chain demand
evidence at 1–3 out of 5 across every candidate thesis. Bound has users and a grant;
it does not yet have someone who has said in writing that they will pay for a bonded
agent. That is the single thing that most changes Bound's fundability, and it is not
something more code produces.

---

## 4. Roadmap

Each milestone lists the evidence that closes it, not the work that fills it.
A milestone is done when a stranger can check it.

### M1 — The rail reaches the reserve `next`

The boundary works; it does not yet land in a bond. Deploy the contracts against
the anchor's USDC as a second deployment, so a SEP-24 deposit funds a certificate's
reserve and a proven claim withdraws to fiat. Then both directions unattended
rather than hand-clicked, and SEP-45 contract-account auth so the protocol
authenticates _as itself_ when settling its own obligation instead of borrowing an
operator's keypair.

**Evidence:** a deposit and a withdraw transaction ID with matching on-chain
reserve deltas, and `check:anchor` reporting the two issuers as the same asset
rather than as two.

### M2 — One named payer `the one that matters`

A single business or agent operator that has committed in writing to bonding an agent
through Bound. Pilot terms, not a letter of interest.

**Evidence:** a signed pilot, and their certificate live on mainnet or testnet with
their name on it.

### M3 — Mainnet, scoped `gated on M2`

Mainnet deployment against a production anchor's USDC, deliberately small: capped
bounds, a published incident policy, and the disclosed defects either fixed or
explicitly accepted in writing. An unbounded mainnet launch of a protocol whose whole
premise is bounded downside would be self-refuting.

**Evidence:** mainnet contract IDs, a bound cap in the contract, and a public
post-mortem policy written before it is needed.

### M4 — TRY receive-side rail `the regional thesis`

The costed answer to section 3. Not "someone should build a TRY anchor" but: this is
the licensing posture, this is the banking partner shape, this is the SEP surface, this
is what it costs and who has to say yes.

**Evidence:** a written specification a prospective anchor operator could act on, and
at least one regulated counterparty who has read it and said what would have to change.

---

## 5. What funding is actually for

| Line                       | Why it cannot be absorbed                                                                                                              |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| Independent security audit | Bound's guarantee is economic. An unaudited slashing contract holding real money is the one thing the thesis cannot survive            |
| Auditor-side liquidity     | The auditor role only works if someone can afford to stake. Bootstrapping the first auditors is a capital cost, not an engineering one |
| Regional rail work (M4)    | Licensing and banking conversations are slow, unglamorous, and not something a solo builder does between features                      |

> **Unfilled:** the amount and the SCF tier. Stated out loud rather than left in
> an HTML comment, because a comment renders as nothing and a blank nobody can
> see is a blank that ships.

---

## 6. Asks

In priority order, and deliberately not all money:

1. **A named pilot or payer.** Worth more than any prize in this event. Section 3 says
   why, and a room of founders and operators is exactly where one comes from.
2. **An SDF contact who owns anchor and SEP strategy for the region.** M4 is
   unfundable and probably unbuildable without one.
3. **Clarity on how InstAward and SCF compose** for a product that already has an
   award delivered against it.

---

## 7. Why this is a different bet from a new team

Bound's ancestor shipped on another chain before this one — same thesis, one chain
earlier. The problem has been worked on longer than this hackathon, longer than the
grant, and the parts that are hard about it are already known rather than about to be
discovered. A from-scratch team pitching the same idea would be pitching the version
of it that has not met reality yet.
