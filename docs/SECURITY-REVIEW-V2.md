# Adversarial security review — Bound v2 contract set

Reviewer: an adversarial pass over the combination, not over the pieces.
Base: `origin/main` @ `6c2f81a`. Branch: `review/v2-adversarial`.

## Scope and method

The v2 contract set was written in one session by several agents, each reviewing
only its own contract. The individual mechanisms are sound; this review looks
only at where they meet. In scope: `registry`, `reserve-vault`,
`auditor-staking`, `challenge-manager`, `payment-router`, `premium-vault`.
`fee-escrow` is dead code off the settlement path and `spend-probe` is a
non-deployable spec; neither was reviewed beyond confirming that.

Method: read all six contracts end to end, then attack the five load-bearing
claims — "manufacturing a true proof does not pay", "the spend counter is
evidence, not a payout trigger", "nothing can mint", "ordering is worthless",
"the auditor's capital is bounded per certificate" — by writing the sequence out
with numbers and then trying to make it run in the offline cross-contract
harness. Four attacks ran. They are section 14 of
`contracts/integration-tests/tests/cross_contract.rs`, and every one of them
**passes**, which is to say the behaviour they describe is real.

Findings already recorded in DESIGN-V2 §10 or THREAT-MODEL's "known gaps" are
not repeated. Where a finding sharpens one of those entries, it says so and says
what is new about it.

Nothing here breaks claim 3 ("nothing can mint") or claim 5 ("the auditor's
capital is bounded per certificate"). Two findings break claim 1 sideways — not
by making a manufactured proof _pay_, but by making a destructive one _free_ —
and one breaks claim 4 in a way the design anticipated only half of.

---

## R1 — A lawful reserve withdrawal manufactures a free, total slash of the auditor's allocation

**Severity: High.** Demonstrated:
`a_lawful_reserve_withdrawal_manufactures_a_free_total_slash_of_the_allocation`.

### Mechanism

`reserve_shortfall` is two reads and a subtraction:

```rust
let claimed: i128 = registry.get_cert_reserve(cert_id);   // immutable, set at publish
let actual:  i128 = vault.get_balance(cert_id);           // zeroed by release_to_operator
if claimed > actual { claimed - actual } else { 0 }
```

It asks nothing else. Not whether the certificate has expired, not whether the
settlement deadline has passed, not whether the withdrawal that emptied the
vault was the one the protocol itself sanctioned. `ReserveVault::release_to_operator`
sets `Balance(cert_id)` to zero at `expires_at + CHALLENGE_WINDOW_SECONDS`, and
`Certificate.reserve_amount` never changes. From that instant the predicate is
permanently true and its `harm` is the full claimed reserve.

The allocation is still there because releasing it is a _separate call the
auditor has to make themselves_ (`AuditorStaking::release_allocation`), unlocking
at exactly the same timestamp as the operator's withdrawal. There is no atomic
unwind and no ordering requirement between the two.

Filing the claim then re-freezes the certificate through
`Registry::set_claim_freeze`, and `release_allocation` reads the live deadline —
so the auditor's escape hatch is nailed shut by the very claim they need to
escape.

### The sequence, with numbers

Standard harness certificate: bound $5,000, claimed reserve $1,000, auditor
allocation $600, `expires_at = 10_000`, `SETTLEMENT_DEADLINE = expires_at + 7d`.

1. Everything goes right. The operator funds the full $1,000. The auditor
   attests with a $600 allocation. The certificate runs its term. No breach, no
   challenge, no complaint.
2. `t = SETTLEMENT_DEADLINE`. The operator calls `release_to_operator` — the
   call the protocol explicitly invites at this instant. Vault balance for the
   certificate: **$0**. Claimed reserve on the certificate: still **$1,000**.
3. The auditor has not yet sent their own `release_allocation` transaction.
   Allocation: **$600**.
4. **Anyone** — the operator, a competitor, a bored address — calls
   `challenge(cert_id, InsufficientReserve, bond = $100)`. The predicate is true:
   `proven = true`, `harm = $1,000`. A 72-hour window opens and the certificate
   freezes. `release_allocation` now reverts.
5. 72 hours later, anyone calls `close_window`.
   `payable = min($1,000, $0 + $600) = $600`; `victim_pool = min(payable, $0) = $0`;
   `fee_pool` is clamped to the empty reserve, so **$0**;
   `slash = min($600 - $0, $600) = $600 → treasury`.

### Cost to the attacker vs cost to the victim

|                   |                                                                   |
| ----------------- | ----------------------------------------------------------------- |
| Attacker pays     | gas, plus $100 of bond float for 72 hours                         |
| Attacker receives | the bond back in full (`ChallengeWins` returns it) — **net zero** |
| Auditor loses     | the **entire $600 allocation**, to the treasury                   |
| Victim receives   | **$0** — the reserve is empty                                     |
| Operator loses    | nothing; they already have their money                            |

### Why the waterfall does not stop it

It is designed to stop attacks that _pay_. This one does not pay, and that is
exactly why it slips through: it is pure destruction. Every rail holds and the
auditor is still wiped out.

### Why this is not DESIGN-V2 §10's known residue

§10 and THREAT-MODEL both describe an operator who "deliberately under-funds
their own certificate and then challenges it", and price it as costing the
attacker "their gas, their reserve and their certificate". None of those costs
are present here:

- No under-funding. The certificate was **fully funded for its entire life**.
- The operator does not lose their reserve — they lawfully withdrew it first.
- The certificate is already expired and worthless, so losing it costs nothing.
- The attacker need not be the operator, or collude with one.

The known residue is an operator burning their own capital to hurt an auditor.
This is a _free_ action available to _anybody_, on _every honestly completed
certificate_, in the window between two independent transactions. It is not a
variant of the known item; it is a different attack that happens to use the same
predicate.

### Suggested direction (not implemented — this is a review)

The obvious rail is to make the predicate stop being true when the covenant has
lawfully ended: refuse an `InsufficientReserve` filing once
`now >= get_cert_settlement_deadline(cert_id)`, or once the reserve has been
released. `verify_expired_certificate` already takes a guard of this shape
(`cert_is_verified`, supersession); `reserve_shortfall` takes none. Whichever
guard is chosen has to be at _filing_ as well as at the live re-read, or the
window still opens.

---

## R2 — Sybil claims dilute an honest victim arbitrarily, and every sybil bond comes back

**Severity: High.** Demonstrated:
`sybil_claims_dilute_an_honest_victim_and_every_sybil_bond_comes_back`.

### Mechanism

DESIGN-V2 §1 chose an **equal split** for the `InsufficientReserve` predicate
group over a sum, explicitly to close _harm amplification_ — `n` copies of one
proof must not drive `payable` to `n × shortfall`. That rail holds. What it does
not close is _harm dilution_.

At close, the certificate-level shortfall is counted once and divided:

```rust
weight = shortfall / shortfall_claims
```

Then every pot is distributed per claim, and each claim pays **the address that
claim named**:

```rust
let share = pool * weights.get(i).unwrap() / total_harm;
pay(&ch, share);   // -> ch.victim  (step 1) / ch.challenger (step 2)
```

Nothing de-duplicates a challenger address, a victim address, or the pair. And
because every one of these claims is _true_, every one is **admitted** — so
`settle_fraud` step 6 returns every one of their bonds in full. The bond is not
a cost; it is float.

### The sequence, with numbers

Certificate claims a $1,000 reserve, holds $900. Genuine shortfall $100.
The auditor's allocation is $600 and untouched here — this attack is against the
_victim_, not the auditor.

Control world, honest claimant alone (this is the existing test
`insufficient_reserve_fraud_pays_victim_and_fee_from_the_operators_own_reserve`):

- victim → **$100**, challenger fee → **$10**.

Attack world. The honest victim files. The attacker then files **three** more
`InsufficientReserve` claims from three throwaway addresses, each naming itself
as both challenger and victim, at the $100 minimum bond:

- `shortfall = $100` (max, not sum — the amplification rail held)
- `shortfall_claims = 4`, so `weight = $25` each, `total_harm = $100`
- `victim_pool = min($100, $900) = $100` → each of the four victims gets **$25**
- `fee_pool = $10` → each of the four challengers gets **$2**, remainder $2 → treasury

|                        |                                                          |
| ---------------------- | -------------------------------------------------------- |
| Attacker pays          | gas, plus $300 of bond float for 72 hours                |
| Attacker receives      | $75 victim compensation + $6 fee + **$300 of bond back** |
| Honest victim receives | **$25 instead of $100** — a 75% haircut                  |

With `n` sybils the honest victim keeps `1/(n+1)`. `n` is the attacker's free
parameter, bounded only by the bond float they can front for three days and by
the transaction cost. Against a certificate with a $100,000 shortfall and a
$100 minimum bond, 99 sybils cost $9,900 of three-day float and capture
$99,000 of a victim's compensation. **The attack scales into profit as soon as
the certificate's reserve is large relative to the minimum bond**, which is the
case the protocol exists for.

### What claim it breaks

Claim 4 says "within a claim window, the order claims are filed must not change
any payout." That is true and I could not break it. But the design's stated
purpose for the window is stronger than order-independence — §1 says the point
is that "a minimum-bond self-challenge dilutes an honest claimant by exactly its
own share of harm **and takes nothing else from them**." The headline test
`a_self_challenge_cannot_foreclose_an_honest_claim_in_the_same_window` asserts
that for **one** self-challenge (one of two instead of one of one). It does not
follow that the property survives `n`, and it does not: the self-dealer's
"share" is a number they choose, and the dilution is free because admitted bonds
are refunded.

The first-resolver race was not removed. It was converted from "whoever settles
first takes everything" into "whoever files the most claims takes proportionally
everything", which is cheaper to run and needs no speed.

### Suggested direction

Any of: charge a non-refundable filing fee even on admitted claims; make the
predicate group's share follow _distinct victim addresses_ rather than claims;
or cap the number of predicate claims a window admits. All three have costs, and
picking one is a design decision, not a review decision.

---

## R3 — An ignored `FakeSignature` claim buys a certificate freeze for the price of gas

**Severity: Medium.** Demonstrated:
`an_ignored_fake_signature_claim_freezes_the_certificate_and_refunds_the_bond`.

### Mechanism

DESIGN-V2 §10 prices this griefing surface explicitly: "a griefing surface
priced at one minimum bond for three days of frozen reserve and allocation". The
code does not charge that bond.

`FakeSignature` has no predicate, so `challenge()` records it
`adjudicated = false` and opens a window. If the arbiter never rules, at close it
takes the `Unadjudicated` branch:

```rust
if !ch.adjudicated {
    Self::return_bond(&env, &ch);
    Self::record(&env, challenge_id, &ch, Verdict::Unadjudicated);
}
```

The bond comes back **in full**. That is deliberate and the stated reason is
good — "a claim nobody judged is not a claim the challenger got wrong" — but it
means the griefer's cost is gas plus 72 hours of float, not a bond.

`close_window_early` is the mitigation §10 chose, and it requires
`arbitrated && adjudicated && !proven`, i.e. the arbiter must actually rule.
An arbiter who does not rule is the exact scenario, so the mitigation is
unavailable precisely when it is needed. The one thing that _does_ forfeit a
bond here is the arbiter bothering to reject the claim.

### The sequence, with numbers

Fully funded, attested certificate. Minimum bond $100.

1. Griefer files `FakeSignature`, bond $100. Certificate frozen:
   `release_to_operator` reverts, `release_allocation` reverts, `attest` reverts.
2. Arbiter ignores it. `close_window_early` reverts (`not_arbitrated`).
3. 72 hours later the window closes. Verdict `Unadjudicated`. **Bond returned in
   full.** Freeze lifts, certificate still `Verified`.
4. Repeat. The test runs two full cycles and asserts the griefer's balance is
   unchanged after both.

|                  |                                          |
| ---------------- | ---------------------------------------- |
| Griefer pays     | gas per cycle; $100 float per 72h        |
| Griefer receives | the bond back, every cycle               |
| Operator loses   | use of the reserve, in 72-hour blocks    |
| Auditor loses    | use of the allocation, in 72-hour blocks |

**Honest limit on this one:** it is not a _continuous_ freeze. `challenge()`
reverts once `now >= closes_at`, and joining an open window does not extend
`closes_at`, so there is a gap between one window closing and the next opening in
which the operator or auditor can win the race to release. So this is a
repeatable denial with a race each cycle, not a permanent lock. That is why it is
Medium and not High.

**What is new:** not the existence of the griefing surface — §10 names it — but
its **price**. §10 believes the griefer forfeits a bond. They do not. The fix
`close_window_early` was scoped on that belief, and it only bites against a
griefer unlucky enough to face a responsive arbiter.

---

## R4 — The arbiter can veto the one trustless proof, and the honest challenger pays for it

**Severity: Medium.** Demonstrated:
`the_arbiter_can_veto_a_true_insufficient_reserve_proof_and_burn_the_bond`.

### Mechanism

`resolve_by_arbiter` reaches **any** pending claim in an open window. Its own
doc-comment is careful about one direction and silent about the other:

> WHAT IT CANNOT REACH ... a claim whose on-chain predicate was **false at
> filing** never opened a window and was rejected on the spot ... no human may
> declare a breach they say did not happen.

The converse is unguarded. There is no check on `ch.proof_type`, so the arbiter
may take an `InsufficientReserve` claim the vault itself proved true and write
`proven = false, harm = 0, arbitrated = true, adjudicated = true` over it. That
claim is now "rejected", which makes it a valid key to `close_window_early`, so
the arbiter can also end the window on the spot.

### The sequence, with numbers

Certificate claims a $1,000 reserve, holds **$0**. Unambiguous arithmetic fraud.
Auditor allocation $600.

1. Honest challenger files `InsufficientReserve`, bond $100.
   `proven = true, harm = $1,000`.
2. Arbiter calls `resolve_by_arbiter(id, false, 0)`. Nothing checks them against
   the vault.
3. Arbiter calls `close_window_early(id)`. The only claim in the window is now
   "rejected", so the rail permits it.
4. Result: certificate status still **`Verified`**. Allocation still **$600**,
   unslashed. Treasury **$0**. Victim **$0**. Freeze lifted. Challenger's **$100
   bond forfeited** to the hygiene pool, verdict `ChallengeFails`.

|                         |                                                                  |
| ----------------------- | ---------------------------------------------------------------- |
| Arbiter gains           | nothing directly — the bond goes to the bounty pool, not to them |
| Honest challenger loses | their bond, for being demonstrably right                         |
| Counterparties lose     | a fraudulent certificate keeps reading `Verified`                |

This is censorship, not theft, which caps the severity. But it is a trust the
docs do not claim. DESIGN-V2 repeatedly calls `InsufficientReserve` "the
protocol's **only** trustless proof" and §9 treats defeating it as the sharpest
defect on the list. It is trustless up to an arbiter veto, and a reader of
`resolve_by_arbiter`'s comment would conclude otherwise. A second-order effect:
the arbiter can make honest challenging expensive, which suppresses exactly the
behaviour the protocol depends on.

The narrow fix is one line — refuse `resolve_by_arbiter` on a claim whose
predicate adjudicated itself at filing, or at minimum refuse to _lower_ a
predicate-computed `proven`. Whether the arbiter _should_ have a veto is a
design call; the point of this finding is that it is currently undocumented and
free.

---

## D1 — `attest` does not verify the reserve, and two documents say it does

**Severity: Medium (documentation/code divergence).** Demonstrated by existing
tests, no new test needed.

DESIGN-V2 §4 states the decision as built: "**`attest` verifies the reserve
before recording the attestation.** `attest` performs the same reserve check
`verify_insufficient_reserve` performs, against the certificate's vault, and
rejects if the vault does not hold at least the claimed reserve." THREAT-MODEL
then leans on it: "The minimum-reserve check at `attest` mitigates the
attestation-time version of this — an auditor cannot be walked into a
certificate that is already fraudulent".

`Registry::attest` makes exactly two cross-contract calls:
`AuditorStaking::is_registered` and `AuditorStaking::allocate`. It never reads
the vault. There is no reserve check.

This is not a subtle inference — the harness's own `staked_and_attested()`
helper publishes a certificate claiming a $1,000 reserve, deposits **nothing**,
and attests successfully. Half a dozen existing tests are built on it, including
`insufficient_reserve_slash_goes_to_the_treasury_and_never_to_victim_or_challenger`,
which runs the full §4 griefing sequence to completion and slashes the auditor's
whole allocation.

So §4's "instant-loss trap" is fully open, and both design documents describe it
as closed. The practical consequence is that an auditor's only defence is to
check the vault balance off-chain before signing — which is a procedure, not a
control, and nothing in the contracts or the SDK enforces it.

I did not check whether §3's vault allowlist is implemented for the same reason
it does not matter to settlement: the ChallengeManager reads the vault address
from **its own** `initialize`, never from `cert.reserve_vault_contract`. The
operator-supplied vault field is therefore inert on the proof path — §3's attack
is neutralised, but by accident of wiring rather than by the allowlist §3
specifies, and a future change that starts reading the certificate's field would
silently re-open it. Worth a comment on the field at minimum.

---

## T1 — No contract in the set ever extends a storage TTL

**Severity: Medium (availability). Suspicion, clearly labelled — not
demonstrated in a test.**

`grep -rn 'extend_ttl\|bump_' contracts/*/src/*.rs` returns nothing. Every
piece of money state in this protocol lives in `persistent` storage and is never
bumped:

- `ReserveVault::Balance(cert_id)`, `Locked`, `UnlockAt`
- `AuditorStaking::Stake`, `Allocated`, `Allocation(cert_id)`, `AllocationAuditor`
- `Registry::Certificate(cert_id)`, `AgentCert`, `ClaimFreeze`
- `ChallengeManager::Challenge(id)`, `Window(cert_id)`, `Settled(cert_id)`
- `PremiumVault::Coverage(cert_id)`

This protocol's defining behaviour is locking capital for a long time and then
not touching it: a certificate can have a year-long term, and its reserve and
allocation are then untouched from `deposit`/`attest` until
`expires_at + 7 days`. That is exactly the access pattern Soroban's state
archival is designed to catch. Archived persistent entries are restorable rather
than lost, so this is availability and operational cost, not loss of funds — but
"the operator must construct a restore footprint before they can reclaim their
own reserve" is not a property anyone has written down, and `Settled(cert_id)`
being archived while `Challenge(id)` is not would be a genuinely confusing state
to reason about.

I have not demonstrated this. The harness `Env` does not exercise archival, and
building a test for it would mean driving `set_min_persistent_entry_ttl` and
ledger sequences in a way no existing test does. **It is a suspicion based on
the absence of any TTL management at all, not a proven defect.** It is on the
list because the absence is total and undiscussed: no doc in `docs/` contains
the word.

---

## Low-severity notes

Recorded so the next reviewer does not spend time on them, not padded into
findings.

- **`verify_bound_exceeded` has none of the guards `verify_expired_certificate`
  has.** No `cert_is_verified`, no supersession check. Because `enroll` is
  permanent, an agent renewed onto a fresh certificate keeps metering onto the
  **old** one, whose counter eventually passes its bound. Anyone can then file
  `BoundExceeded` against the dead old certificate and collect the flat $10
  hygiene bounty out of the forfeited-bond pool. `Settled(cert_id)` caps it at
  one bounty per certificate, so it is a bounded drain — $10 per superseded
  certificate — for no protocol service rendered. The asymmetry between the two
  predicates looks like an oversight rather than a decision.
- **An arbiter-stated `harm` near `i128::MAX` wedges a window.**
  `fee_pool = total_harm * CHALLENGER_FEE_BPS / 10_000` has no `checked_mul`, and
  `overflow-checks = true` is set workspace-wide, so `close_window` panics
  forever and the certificate can never take another claim. The collateral still
  unwinds on the ordinary deadline once `closes_at` passes, so nothing is
  permanently locked. Reachable only by the arbiter, i.e. a trusted-party
  footgun. `PremiumVault::price` uses explicit `checked_mul` for exactly this
  class of input; `settle_fraud` does not.
- **Step 4's premium is genuinely additional to `reserve + allocation`.** Claim 3
  as usually stated ("no path pays out more than reserve + allocation for that
  certificate") is literally false: total outflow is
  `victim_pool + fee_pool (≤ reserve) + slash (≤ allocation) + forfeited premium`.
  This is deliberate and reasoned at length in §10 — the premium is the
  operator's own money and carries its own `victim_cap` — so it is not a defect.
  It is worth stating precisely because the shorthand invites a future reader to
  "simplify" one of the two caps away, which is the risk §10 itself names.

---

## What I tried and could not break

As valuable as the findings. Each of these was attacked deliberately and held.

**Per-certificate isolation (claim 5).** I could not reach one certificate's
collateral from another. `ReserveVault::pay_from_reserve` checks
`amount > Balance(cert_id)` per certificate; `AuditorStaking::slash_allocation`
checks `amount > Allocation(cert_id)` and decrements `Allocation`, `Allocated`
and `Stake` consistently; `PremiumVault` keys `Coverage(cert_id)`. Every
`invoke_contract` on the settlement path passes `cert_id` as its first argument
and none of them takes an address from the challenge. The existing isolation
tests are correct and I found nothing they miss.

**Minting (claim 3).** `distribute` truncates every share, so the shares sum to
**at most** the pool, never more; the caller routes the shortfall to the
treasury, so nothing is stranded either. I checked the same pattern in
`settle_hygiene` (`bounty / admitted.len()`, remainder stays in the pool) and in
step 4's premium fan-out. `bounty_pool = balance - bonds_held` clamps at zero and
`release_bond_liability` clamps at zero, so a drift in `BondsHeld` can only ever
_understate_ the surplus. I could not find a path that pays a bond out twice or
pays a hygiene bounty out of live bond money: each challenge id appears in
exactly one `ClaimWindow`, the window is removed before settlement, and
`Settled(cert_id)` forecloses a second one.

**Double payment across the window, the premium vault, and bond returns.** A
victim can be paid from step 1 and again from step 4, but the two are capped by
`victim_pool` and `victim_cap = total_harm - victim_pool` respectively, which sum
to exactly `total_harm`. `PremiumVault::forfeit` returns precisely what it
transferred, so the ChallengeManager's fan-out cannot over-distribute. `forfeit`
and `terminate` both early-return on `c.closed`, so the premium pot cannot be
harvested twice.

**Cross-contract reentrancy and partial state.** Not reachable. Settlement is
one Soroban transaction and a panic anywhere in `settle_window` reverts all of
it, so there is no half-settled state to find. The outbound calls are to the
vault, staking, the registry and the premium vault — all four are contracts in
this set with no callback into the ChallengeManager — and the only calls to
_attacker-influenced_ addresses are SAC `transfer`s to `ch.victim` and
`ch.challenger`, which do not invoke contract code on Soroban. I looked
specifically for a victim address that could re-enter `close_window` mid-fan-out
and there is no such path.

**Ordering (claim 4).** Genuinely order-independent, and more carefully than it
needed to be. `shortfall` is a `max` and `shortfall_claims` a count, both
commutative; all weights are computed before any payment; `pool` and `total_harm`
are fixed before `distribute`'s first transfer; per-share truncation does not
depend on position; the hygiene split is equal. The remainder-to-treasury rule
really does remove the tie-break. I could not construct two orderings with
different payouts. (R2 is not an ordering attack — it works identically in any
order.)

**Curing and the filing-time record.** The §2 asymmetry holds:
`ch.proven`/`ch.harm` are written once in `challenge()` and no later path
recomputes them; `live_predicate` is only ever consulted to _spare_ a
certificate and return a bond. I could not find a way for an operator to move
money in their own favour by changing state mid-window.

**Freeze vs the TTL/settlement-deadline locks.** The one-mechanism decision pays
off. `get_cert_settlement_deadline` is `max(expires_at + 7d, claim_freeze)` and
both `release_to_operator` and `release_allocation` take `max(snapshot, live)`,
so I could not find a stale snapshot that lets collateral escape an open window.
The reverse — a freeze that outlives its window — is also closed: `settle_window`
calls `freeze(cert_id, 0)` on every exit path including the empty-admitted one.

**`clawback` and the spend meter.** The meter is deliberately not touched by
`clawback`, which is right: metering it would let an operator inflate their own
`spent` and make a recovery indistinguishable from a `BoundExceeded` breach. I
tried to make a recovery look like a breach and could not — `clawback` is a pure
internal balance move, `Supply` is untouched, the float moves by the same rule
the hot path uses, and routing stays halted so the sweep implies no resume. I
also could not make a breach look like a recovery: `spent` only moves through
`meter`, which only runs for an enrolled `from`.

**Enrollment as a source of truth.** Permanent, dual-signed, and I could not
attach spend to a stranger's certificate or walk an agent off a climbing counter
(`already_enrolled` panics on any re-bind, same certificate or not).

**Hygiene mode not slashing.** I specifically tried to find a route by which
`BoundExceeded` or `ExpiredCertificate` reaches a slash on the counter alone.
There is none: both record `harm = 0` at filing, and the only way to attach a
number is `resolve_by_arbiter`, which is the documented and intended trust. The
`spend-probe` principle is correctly implemented.

**Auth.** Every privileged entry point I could find is gated on the right party:
`pay_from_reserve`/`slash_allocation`/`retire_allocation` on the ChallengeManager,
`allocate` on the Registry, `set_claim_freeze`/`invalidate` on the ChallengeManager,
`forfeit`/`terminate` on the ChallengeManager, `set_router`/`set_premium_vault`
on the arbiter and one-shot, deposits and withdrawals on the certificate's live
operator read from the Registry. `initialize` being unauthenticated everywhere is
the known, deliberate defect and I found nothing worse of that shape. The one
authority that is broader than its documentation is R4.

---

## Summary

| #   | Finding                                                         | Severity | Demonstrated      |
| --- | --------------------------------------------------------------- | -------- | ----------------- |
| R1  | Lawful reserve withdrawal manufactures a free total slash       | High     | yes               |
| R2  | Sybil claims dilute an honest victim; admitted bonds refunded   | High     | yes               |
| R3  | Ignored `FakeSignature` claim freezes for gas, bond refunded    | Medium   | yes               |
| R4  | Arbiter can veto the trustless proof and burn the honest bond   | Medium   | yes               |
| D1  | `attest` reserve check documented in two places, absent in code | Medium   | by existing tests |
| T1  | No TTL management anywhere                                      | Medium   | no — suspicion    |

R1 and D1 compound: D1 leaves the auditor exposed at attestation, and R1 leaves
them exposed at the end of the certificate's life. Between them, an auditor's
allocation is destroyable by a third party at both ends of a certificate that
never did anything wrong.

The waterfall itself is good work. Every finding above is a case where the
waterfall does what it was designed to do — nobody is enriched — and the harm
lands anyway, because the design's threat model is built around attacks that
pay. The gap in the model is the attacker who is content to burn someone else's
money for gas.
