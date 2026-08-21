# Bound v2 — resolved design decisions

Status: **decided, not implemented.** This document closes the eight problems the
adversarial review left open, plus a ninth the integration harness found. Each entry states the decision, the alternatives
rejected and why, and the test that will prove it. It is the input to the v2
contract work and the first thing an auditor should read.

Nothing here is deployed. Where a decision contradicts a v1 behaviour, the v1
behaviour is a known defect scheduled for the v2 redeploy, not a regression.

---

## The principle the whole document rests on

`spend-probe` (`contracts/spend-probe/`) proves that cumulative routed spend
measures **gross flow**, not **loss**. A $1 shuttle between two addresses the
operator controls drives spend past any bound for the price of gas.

So the settlement rule for every proof type is:

> A proof establishes that **the covenant was broken**. It does not establish
> that **anyone was harmed**. Compensation is driven by proven harm, capped by
> collateral. The counter is evidence, never a payout trigger.

Every decision below is downstream of that sentence.

---

## 1. First-resolver-takes-all — CRITICAL

**Status: implemented, not deployed** (branch `feat/claim-window`). Landed
together with §2, because a claim window without filing-time evaluation is a
strictly worse protocol than what it replaced.

**Problem.** If one settlement extinguishes a certificate, a self-challenge for
the minimum bond permanently forecloses every honest claim behind it. The attack
costs the minimum bond and destroys the entire coverage.

**Decision. A claim window that aggregates before paying.**

A first valid challenge opens a fixed **claim window** — `CLAIM_WINDOW_SECONDS`,
**72 hours** of ledger time — rather than settling. During the window any party
may file an additional claim against the same certificate. The certificate is
frozen: no new attestation, no reserve withdrawal, no allocation release, no
expiry-based escape. At window close, settlement runs **once**, over all
admitted claims:

- Claims are paid **pro rata** from the available collateral when total proven
  harm exceeds it, in full when it does not.
- Pro rata, not first-come — ordering within a window must not be worth anything,
  or we have rebuilt the race we are removing.

### What was built

`ChallengeManager::challenge` no longer settles. It evaluates the predicate
(§2), then either **opens** a window (`DataKey::Window(cert_id)`, a `ClaimWindow`
holding `closes_at` and the list of claim ids) or **joins** the open one.
`ChallengeManager::resolve` is **gone**. In its place:

`close_window(cert_id)` — **permissionless**, callable by anyone once
`closes_at` has passed. **The caller pays the transaction fee, and there is no
reward for making the call.** A bounty here would be a second pot to game, and
none is needed: nobody in the window is paid a stroop until somebody calls it,
so every claimant is motivated to be that somebody and any of them can be. The
certificate stays frozen until it happens, so an unclosed window costs the
operator and the auditor rather than the victims.

**The freeze reuses the existing settlement-deadline lock rather than inventing
a second mechanism.** The Registry already publishes
`get_cert_settlement_deadline(cert_id)`, and both the ReserveVault and
AuditorStaking already refuse to release before it. The ChallengeManager now
calls `Registry::set_claim_freeze(cert_id, closes_at)` when it opens a window,
and the deadline becomes `max(expires_at + CHALLENGE_WINDOW_SECONDS,
claim_freeze)`. One mechanism, three enforcement points, no way for them to
drift apart. Two follow-on changes were required: `release_to_operator` and
`release_allocation` both used to trust a _snapshot_ of the deadline taken at
deposit/attest time, which cannot know about a window opened later, so both now
read the live value and take the later of the two. `attest` refuses a frozen
certificate outright, so an operator cannot walk a fresh auditor onto a
certificate that has already been filed against.

### How harm aggregates, and why it is not a sum

This is the part a reader will want to "simplify", and it must not be.

- A harm the **arbiter stated** is an assessment of what **one claimant** lost.
  Two claimants who each lost $500 lost $1,000 between them. These **sum**.
- A harm a **predicate computed** is a property of the **certificate**.
  `InsufficientReserve` reads one shortfall off one vault; ten people noticing
  the same $800 hole have not proven $8,000 of harm. So the shortfall is counted
  **once** — the maximum recorded across the window's filings, i.e. the worst
  state the certificate was in — and **shared equally** by the claims standing
  on it.

**The attack this closes is harm amplification.** If identical predicate claims
summed, anyone could file _n_ copies of the same true proof and drive `payable`,
and with it the auditor's slash, to _n_ times the real shortfall — up to the
whole allocation — for _n_ minimum bonds. The waterfall's "capped by proven
harm" rail would be capped by proven harm times a number the attacker picks.
Equal shares within the predicate group rather than pro rata, because the
predicate cannot tell the claimants apart: it reads the vault, not the victims.
Equality is also the only order-independent answer.

**The attack this did NOT close is harm dilution, and the answer is R2.** Equal
shares stop `n` copies of a proof enlarging the slash. They did not stop `n`
copies of a proof taking `n/(n+1)` of the **victim pool** away from an honest
claimant: nothing de-duplicated a challenger or a victim address, and an
admitted claim gets its bond back in full, so the sybil's cost was gas plus 72
hours of bond float. Address de-duplication is not a fix — a sybil farms
addresses. Neither is a non-refundable filing fee, nor a cap on claims per
window; the first prices the attack, the second lets the attacker fill the cap
and foreclose the honest claimant outright.

The fix is to stop asking the predicate a question it cannot answer. **A
predicate-computed proof establishes that the covenant was broken, not that any
particular person was harmed.** That is this document's own founding principle,
and it is precisely why `BoundExceeded` and `ExpiredCertificate` settle in
hygiene mode with no victim payment at all. `InsufficientReserve` was treated
differently only by inheritance from v1, and the dilution attack is the
consequence: the contract was trying to compensate victims it has no way to
identify, so whoever showed up in the largest numbers took the pot.

So **`InsufficientReserve` pays no victim compensation.** The operator's reserve
is still drawn — the operator does not keep money they failed to commit — but
the draw goes to the **treasury**, which is not a party to the window and cannot
be sybilled into being one. The challenger is still paid their fee. Victim
compensation flows only from **arbiter-assessed harm**, the one mechanism that
can name and size a victim. The equal split above survives unchanged; it now
sizes the slash and the fee, and nothing else.

Mechanically, the victim pot is `min(reserve_draw, arbitrated_harm)` and is
divided by **assessed** harm, not by `total_harm`. Dividing by `total_harm`
would let a single free predicate claim shave the assessed victim's share and
send the difference to the treasury — a smaller attack, but the same one. With
the assessed denominator the assessed victim's payout does not depend on how
many predicate claims stand beside it, at any `n`.

**THE COST, STATED PLAINLY, AND IT CONTRADICTS EARLIER FRAMING IN THIS
DOCUMENT.** The protocol's only trustless proof no longer compensates anyone
directly. The reserve still covers proven harm, but **proving harm now always
requires the arbiter**. This narrows the trustless surface, and it makes §10's
R4-style arbiter veto a wider trust than it was. Everywhere above that describes
`InsufficientReserve` as paying a victim describes the pre-R2 contract.

**Rejected — anything that merely raises the attacker's cost.** A half-measure
that makes the sybil pay more is not a fix; `n` is the attacker's free parameter
and the certificates worth attacking are the ones where the pot dwarfs the bond.

### Rounding: the remainder goes to the treasury

Every pro-rata share is `pool * weight / total_harm`, truncating, so the shares
can sum to a stroop or two under the pool. **The remainder goes to the
treasury**, not to a claimant. Handing it to "the largest claim" or "the first
claim" would make a payout depend on a tie-break, and a tie-break is ordering
value — the exact thing the window exists to destroy. The treasury is not a
party to the window and cannot be gamed into being one. The amount is bounded by
(claims − 1) stroops, 10⁻⁷ USDC each. Nothing is stranded and nothing is
conjured: the total leaving the reserve is exactly the pot that was sized for
it, and the pot is still capped by `payable = min(total_harm, reserve +
allocation)`.

The hygiene bounty is the one pot that is **not** pro rata: hygiene harm is zero
by definition, so there is no ratio to divide by. It is **one** flat $10 bounty
for the window, split **equally** — its job is to pay for the gas of killing a
dead certificate, and that job is done once however many people showed up to do
it. Otherwise filing _n_ copies of the same hygiene proof would mint _n_
bounties.

**Rejected — multi-claim settlement without a window.** Pay each valid claim as
it resolves until collateral is exhausted. Simpler, but it keeps the race: the
attacker still front-runs honest claimants, they just drain rather than
extinguish. It makes the ordering profitable, which is the actual defect.

**Rejected — one certificate, one claim, ever.** This is v1. It is the bug.

**Cost, stated plainly.** A genuine victim now waits out the whole window before
being paid a stroop, even when theirs is the only claim ever filed. That latency
is real and it is the price of not letting a self-dealer foreclose them. It
belongs in the trust model as a known latency, not hidden. 72 hours is a
proposal, not a researched number.

**Tests** (`contracts/integration-tests/tests/cross_contract.rs`, section 12).

- `two_claimants_with_enough_collateral_are_both_paid_in_full`
- `two_claimants_short_of_collateral_are_paid_pro_rata_to_the_stroop` — the sum
  paid equals the available collateral exactly.
- `the_pro_rata_remainder_goes_to_the_treasury_and_never_goes_missing`
- `a_self_challenge_cannot_foreclose_an_honest_claim_in_the_same_window` — the
  headline. Runs the attack and a control world side by side. Since R2 the
  honest claimant is on the arbiter-assessed path, which is where compensation
  now lives, and the property is stronger than it was: their recovery is
  **identical** with and without the free self-challenge, rather than halved by
  it.
- Section 14: `sybil_claims_dilute_an_honest_victim_and_every_sybil_bond_comes_back`
  (the review's PoC, converted — three sybils take nothing from an assessed
  victim) and
  `a_predicate_proof_pays_no_victim_and_draws_the_reserve_to_the_treasury`.
- `filing_order_inside_a_window_changes_no_payout`
- `a_claim_filed_after_the_window_closes_is_rejected`
- `an_open_window_freezes_the_reserve_and_the_allocation_past_expiry`
- `an_open_window_refuses_a_new_attestation`

---

## 2. Curing a proof mid-challenge — CRITICAL

**Status: implemented, not deployed** (branch `feat/claim-window`), together
with §1.

**Problem.** With a two-phase challenge, an operator can top up the reserve
between filing and resolution, flip the predicate to false, and pocket the
challenger's forfeited bond. The protocol pays the operator for having been
caught.

**Decision. The predicate is evaluated at filing, and a cure returns the bond.**

Two mechanisms, both required, both built:

1. **Evaluate at filing.** `challenge()` computes the predicate and its quantity
   in the same read and records them on the `Challenge` as `proven` and `harm`.
   Nothing downstream ever recomputes whether the challenger was right.
2. **A cure returns the bond.** At window close the live predicate is
   re-read — and it answers a _different question_: not "was the challenger
   right" but "is the certificate still broken". If it is now false, the claim
   resolves `Cured`: bond returned **in full**, certificate survives, nobody
   slashed, nothing forfeited from the premium pot.

The two questions are kept deliberately apart in `close_window`, and the
asymmetry is the safety property: **recorded state decides the bond, live state
decides the certificate, and live state can only ever move in the challenger's
favour.**

`Cured` is a third outcome alongside `ChallengeWins` (upheld) and
`ChallengeFails` (rejected). A fourth, `Unadjudicated`, covers an arbiter-gated
claim the arbiter never ruled on before the window closed: the bond comes back
whole, because a claim nobody judged is not a claim the challenger got wrong.
**`ChallengeFails` is now the only outcome that forfeits a bond**, and it is
reachable only by being wrong at filing.

### Does a cure cost the operator anything? No — and that is a decision

**A cure is free.** The operator restores the reserve, the certificate survives
untouched, the auditor is untouched, and the challenger is made whole out of
their own returned bond. The protocol takes nothing.

This is a deliberate call, and it does under-price getting caught: an operator
can run a persistently underfunded certificate and only ever top it up when
challenged, using the 72-hour window as free credit. Two things make that a
tolerable trade rather than a hole. First, **each cure costs the operator a
challenge's worth of public evidence** — the `Cured` challenge is on-chain
forever and readable by any counterparty. Second, and decisively, **the
alternative prices remediation.** A cure fee would be a tax on the exact
behaviour the protocol wants most, and an operator weighing "fix it and pay" against
"do not fix it and hope" is an operator we have pushed toward the second answer.
A penalty here also needs a recipient, and every candidate recipient — the
challenger, the treasury — turns curing into somebody's revenue line and
re-creates a version of the bounty-hunting incentive §10 spent so much effort
removing.

**What would change the call:** evidence of repeat cures on the same
certificate. A per-certificate cure counter, with a fee or a forced
re-attestation on the second or third, is the obvious next step and is listed in
§10's open gaps. It is not built, because pricing it without a loss history
would be inventing a number.

**Rejected — evaluate at filing only.** Sound against the theft, but it forces a
slash on an operator who fixed the problem within hours. That over-punishes and
discourages exactly the remediation we want.

**Rejected — bond returned on cure only.** Without filing-time evaluation the
operator still controls the predicate at resolution time, so it does not close
the hole; it only makes the theft cheaper.

### Interaction with §1

A cure closes the whole claim window: with no admitted claim left, `close_window`
lifts the freeze, deletes the window and leaves the certificate alive — a fresh
window may open later. Claims filed **after** the cure are rejected rather than
aggregated, and this falls out of mechanism 1 rather than needing its own rule:
a claim filed against a certificate that has already been fixed is _false at
filing_, and false-at-filing is precisely what `ChallengeFails` means.

One consequence worth naming: **a claim that is false at filing with no window
open does not open one.** It is rejected on the spot inside `challenge()` and
its bond forfeited. If a wrong claim could freeze a certificate for 72 hours,
anybody could freeze any certificate for the price of the minimum bond. Once a
window _is_ open a wrong claim is allowed to join it, because there it changes
nothing.

**Tests** (`contracts/integration-tests/tests/cross_contract.rs`, section 13).

- `a_reserve_topped_up_during_the_window_resolves_as_cured` — bond back in full,
  no slash, certificate still Verified.
- `a_claim_filed_after_the_cure_is_rejected_and_forfeits_its_bond` — the cure
  does not launder a bad challenge.
- `state_recorded_at_filing_governs_the_payout_not_live_state` — the operator
  shrinks the live shortfall from $800 to $100 and the settlement still pays
  $800.
- `a_cured_resolution_leaves_the_auditors_stake_and_premium_alone`.

---

## 3. Operator-supplied vault address — CRITICAL

**Problem.** Reading `cert.reserve_vault_contract` lets the operator name a
contract that lies about its balance. Every reserve proof then reads a number the
attacker chose.

**Decision. A registry allowlist of vault implementations, by wasm hash.**

The registry stores a set of approved vault **wasm hashes**. `publish` rejects a
`reserve_vault_contract` whose deployed code hash is not in the set. Allowlisting
the code, not the address, means any operator can deploy their own vault instance
without asking permission — they simply cannot deploy a _lying_ one.

**This reintroduces a permissioned surface, and that is a real cost.** It is
accepted for one reason: the alternative is a proof system reading attacker-
controlled numbers, which is not a proof system. The permission is bounded to
"which code is trustworthy", it is enumerable on-chain, and it is governed by the
same timelock as §5.

**Rejected — a single protocol-wide vault.** No allowlist needed, but it is one
custody blast radius for every operator's money (see §6) and it is what F01 is
trying to get away from.

**Rejected — trust the operator's vault, prove reserve differently.** There is no
way to prove a balance without trusting the contract reporting it.

**Tests.**

- `publish` with an allowlisted wasm hash → succeeds.
- `publish` with a vault whose code hash is not allowlisted → rejected.
- A vault that reports a balance it does not hold, deployed from non-allowlisted
  code → cannot be attached to a certificate at all.
- Removing a hash from the allowlist does not retroactively invalidate
  certificates already published against it (or does — **decide before
  implementing**; the safer default is that existing certificates continue and
  only new publishes are blocked, so removal is not a mass-invalidation weapon).

---

## 4. Griefing an auditor into an instant slash — HIGH

**Problem.** `attest` never checks that the vault actually holds the claimed
reserve. An auditor can be walked into a certificate that is provably fraudulent
the moment they sign it — the operator publishes an underfunded certificate,
waits for attestation, then self-challenges.

**Decision. `attest` verifies the reserve before recording the attestation.**

> **Status: implemented, not deployed** (branch `feat/attest-verifies-reserve`).
> `attest` panics `reserve_not_funded` when the vault holds less than the
> certificate's claimed `reserve_amount`. All five tests below are written, plus
> an exact-boundary test and a per-certificate isolation test.

`attest` performs the same reserve check `verify_insufficient_reserve` performs
and rejects if the vault does not hold at least the claimed reserve. An auditor
cannot sign a certificate that is already fraudulent.

The comparison is `reserve_amount > balance`, character for character the one
`ChallengeManager::reserve_shortfall` uses to decide a shortfall exists. A vault
holding **exactly** the claimed reserve therefore has no shortfall and is
attestable: the boundary belongs to the honest operator in both contracts.
`test_attest_and_the_shortfall_proof_agree_at_the_exact_boundary` pins the two
together so they cannot drift apart, because a disagreement between them would
itself be the defect this section is about.

**Which vault — not the certificate's.** This section originally said "against
the certificate's vault", and that was decided when §3's allowlist was still
expected to constrain `cert.reserve_vault_contract`. **§3 was never built**, so
the operator-named address is unconstrained: an operator could point it at a
vault they funded, or at one that simply lies, and the check would pass while
the certificate settlement actually measures stayed empty — a check that proves
nothing.

So `attest` reads the **ChallengeManager's** vault, obtained live through a new
additive `ChallengeManager::get_reserve_vault()`. That is the vault
`reserve_shortfall` measures and `pay_from_reserve` settles out of, which makes
the two checks agree by construction rather than by convention. The Registry
holds no vault address of its own, so the two cannot drift, and `initialize`'s
argument list — the on-chain ABI the deploy script and the committed bindings
pass positionally — does not have to widen. That is the same reasoning
`set_router` and `set_premium_vault` are built on.

Worth recording plainly: `cert.reserve_vault_contract` is read by **nothing** in
the deployed system. It is a field the operator fills in that no settlement path
consults. §3 is still worth doing, but this fix does not depend on it and no
longer needs to land with it.

This changes `attest` from a registry-local write into a cross-contract call.
That is a real shape change: attestation now depends on the ChallengeManager and
the vault being live and initialized.

**Not sufficient on its own.** It closes attestation-time griefing. It does not
stop the operator withdrawing the reserve _after_ attestation, which is what the
`InsufficientReserve` proof and the auditor's own monitoring are for. The
auditor's risk is real and ongoing; this only removes the instant-loss trap.

**It does not close collusion.** An operator who supplies a lying vault can
still, in principle, walk an auditor in — this fix closes the _accident_, not
the conspiracy. In today's wiring the check reads the protocol's own vault, so
there is nothing for the operator to lie with; that protection comes from the
ChallengeManager holding a single vault address, not from anything §4 enforces.

**A consequence worth naming: the reserve can now barely leave.** `deposit`
locks a certificate's reserve until its settlement deadline and there is no
partial withdrawal, so requiring funding _before_ attestation means an attested
certificate's reserve cannot legitimately move until
`expires_at + CHALLENGE_WINDOW_SECONDS`. The post-attestation withdrawal this
section says §4 does not cover is therefore reachable only after that deadline
(or through a settlement drawing the reserve down). §4 plus the deposit lock is
strictly stronger than §4 alone — but the proof is not vestigial: a post-deadline
withdrawal against an allocation the auditor has not yet reclaimed still upholds,
which `a_reserve_withdrawn_after_attestation_is_still_provable_fraud`
demonstrates.

**A workflow change, and a client-visible one.** The reserve must be funded
before the auditor attests: `publish` → `deposit` → `attest`. A client that
attests first now fails. See `V2-CUTOVER.md`.

**Tests.**

- `attest` on a fully-funded certificate → succeeds.
- `attest` on an underfunded certificate → rejected, and no attestation
  recorded: the certificate is still `Pending`, has no auditor, and the
  auditor's allocation is untouched.
- Exactly equal balance → accepted; one stroop short → refused, asserted
  against the shortfall function's own answer at the same boundary.
- A sibling certificate's reserve does not fund this one.
- Publish underfunded → attest rejected → self-challenge finds no attestation to
  slash. The full griefing sequence fails at step two.
- Fund, attest, then withdraw → the `InsufficientReserve` proof still upholds.
  Post-attestation withdrawal remains slashable.

---

## 5. Admin keys — HIGH

**Problem.** v1's flaw is that nothing can be repointed. Every v2 draft answers
with an admin plus `upgrade()` — a single key that can drain reserves and slash
auditors. The fix is worse than the defect.

**Decision. A timelocked, scope-limited admin with a published policy and a seal.**

Four constraints, all enforced in code rather than by convention:

1. **Scope.** The admin may upgrade contract code and manage the §3 allowlist. It
   may **not** move funds, slash an auditor, resolve a challenge, or invalidate a
   certificate. Those paths take no admin branch at all.
2. **Timelock.** Every admin action is a two-step propose/execute with a
   mandatory delay (proposed: 7 days), so depositors can exit before a change
   takes effect. A change nobody can escape is not governed.
3. **Seal.** The admin can permanently renounce, burning the key. The protocol's
   end state is no admin. The seal is one-way and testable.
4. **Published policy.** What the admin may do, the delay, and who holds the key
   are stated in the docs before mainnet, not after.

**Rejected — no admin at all.** This is v1. Unupgradeable contracts with known
defects and no way to allowlist a new vault. The reason we are redeploying.

**Rejected — admin with full power, "we'll be careful".** The auditor stakes real
money against this system. "Trust us" is precisely what the protocol exists to
replace.

**Tests.**

- Admin attempts to move reserve funds → rejected. Same for slash, resolve,
  invalidate. Each is a separate test; the negative surface is the point.
- Execute before the delay elapses → rejected. Execute after → succeeds.
- Proposal cancelled during the delay → cannot later be executed.
- After seal: every admin action rejected, permanently. Seal cannot be undone.

---

## 6. Custody raises the cost of a stolen agent key — HIGH

**Problem.** Today a compromised agent key drains the agent's wallet. With router
custody it drains the operating float too. The bond is meant to cover exactly
this, but the threat model was never written down.

**Decision. Write the threat model down, and cap the float in code.**

Custody ships **only** with:

1. **A per-certificate float cap.** The router holds at most a stated maximum per
   certificate, set at publish and visible on the certificate. A stolen key
   cannot reach more than that cap, and a counterparty can see the number before
   trusting the agent.
2. **A written threat model**, committed alongside the contracts, stating what a
   compromised agent key can and cannot reach, what the operator can and cannot
   recover, and explicitly that key compromise is **not** a slashable auditor
   fault — the auditor attested to capital and process, not to the operator's key
   hygiene. Getting that boundary wrong makes auditing uninsurable.
3. **An operator kill switch** that halts routing for a certificate without
   requiring a challenge or invalidating the certificate. Compromise response
   must not depend on the challenge system.

**Rejected — ship custody, document later.** The float cap has to be in the
certificate structure at publish. Retrofitting it is another redeploy.

**Tests.**

- Routing a payment that would push the float above the cap → rejected.
- The cap is visible in `get_certificate` and in the SDK's `CertView`.
- Kill switch halts routing immediately and does not invalidate the certificate
  or slash the auditor.
- Kill switch is operator-only; the agent key cannot clear it. (A thief holding
  the agent key must not be able to re-enable routing.)

---

## 7. A $1 post-expiry payment is fatal — HIGH

**Problem.** Under a naive expiry predicate, one small late payment — which a
hostile counterparty can induce — permanently kills an honest agent's
certificate. The attack costs a dollar.

**Decision. A grace window plus a de-minimis floor, with the burden on the
challenger to clear both.**

`ExpiredCertificate` upholds only when **all** hold:

1. The payment settled **after** `expires_at` plus a **grace window** (proposed:
   24 hours), and
2. the payment's value is at least a **de-minimis floor**, defined as a
   percentage of the certificate's bound rather than a flat amount (proposed:
   0.1% of bound), and
3. the certificate had not been renewed or invalidated before the payment.

A percentage floor rather than a flat one, because a flat floor is either
irrelevant at a $1M bound or fatal at a $1k one.

**Both parameters are themselves attack surface, and this is a genuine
trade-off, not a clean win.** A grace window is free post-expiry coverage a
hostile operator can plan around; a de-minimis floor is a band of payments that
provably breach the covenant and are unprovable anyway. The floor is set as a
fraction of bound so the exposure it creates is bounded by the same number the
certificate already advertises. These are transparent published parameters, per
the milestone's stance on pricing — not a risk model.

**Rejected — strict expiry.** Correct in theory, fatal in practice, and cheap to
weaponise.

**Rejected — grace window only.** A $1 payment one second after the grace window
is still fatal. Moves the cliff without removing it.

**Tests.**

- Payment inside the grace window → proof rejected.
- Payment after grace, below the floor → rejected.
- Payment after grace, above the floor → upheld.
- Payment after grace, above the floor, on a renewed certificate → rejected.
- Floor scales with bound: the same absolute payment is below the floor on a
  large bound and above it on a small one.

---

## 8. Enrollment is optional — MEDIUM

**Problem.** An agent that never calls `enroll` is untracked, yet
`verify().valid` still returns true. The most reassuring answer the protocol
gives is available to precisely the agents it is not watching.

**Decision. `verify` reports tracking status; validity requires it.**

Both halves:

1. `VerifyResult` gains an explicit **`tracked`** field. No caller has to infer
   it, and the SDK surfaces it in `CertView` so the marketplace can show it.
2. `valid` requires `tracked`. An untracked certificate is not valid, because
   `valid` means "a counterparty may rely on this", and nothing that is not being
   watched should be relied on.

Reporting alone was tempting — it is non-breaking and lets a caller decide. It is
rejected because the whole value of a single `valid` boolean is that a
counterparty does not have to know the protocol's internals to use it safely. A
`valid: true` that means "valid, but we aren't watching" is a trap for exactly
the least sophisticated integrator.

**Tests.**

- Published and attested but never enrolled → `valid == false`, `tracked ==
false`, `status == Verified`. The three fields must be independently legible.
- Enrolled → `valid == true`, `tracked == true`.
- The SDK's `toCertView` carries `tracked` through to `CertView`.

---

## 9. The reserve vault is shared, and it defeats the one trustless proof

**Not from the adversarial review — found by the integration harness**
(`defect_reserve_vault_balance_is_shared_across_certificates`), and it is the
sharpest defect on the list.

**Problem.** `ChallengeManager::verify_insufficient_reserve` reads the vault
balance with **no certificate argument**:

```rust
let actual: i128 = env.invoke_contract(
    &reserve_vault, &Symbol::new(env, "get_balance"), Vec::<Val>::new(env));
actual < claimed
```

`ReserveVault` stores a single `DataKey::Balance`, and the challenge manager
holds one vault address from its own `initialize`. So a single pooled balance is
compared against one certificate's claimed reserve. Any deposit — from any
certificate, by any operator using that vault — makes an entirely unfunded
certificate read as fully backed. The harness proves it: an unrelated deposit
flips a genuine fraud proof to `ChallengeFails` and the auditor keeps their stake.

`InsufficientReserve` is the protocol's **only** trustless proof today. This
defect defeats it.

**Decision. This is F01, and it is why the per-certificate refactor is a
prerequisite rather than an enhancement.**

> **Status: implemented, not deployed** (branch `feat/per-certificate-reserve`).
> The vault now keys `Balance`, `Locked` and `UnlockAt` by certificate id and
> stores no global operator; `deposit` and `release_to_operator` authenticate
> against `Registry::get_cert_operator(cert_id)`. The harness test that used to
> document the defect is inverted: `unrelated_deposit_does_not_rescue_an_
unfunded_certificate` asserts the fraud proof now upholds and the auditor is
> slashed. The contracts running on testnet are still v1 and still carry the
> defect described below.

Reserve accounting becomes per-certificate: `get_balance(cert_id)`, deposits
attributed to a certificate, and the proof reading the certificate's own vault
(constrained by the §3 allowlist, since the operator names it). No new mechanism
is decided here — §3 and the F01 refactor already cover it. What is new is the
evidence that shipping the premium economy or the new proofs on top of v1's
shared vault would make the system strictly more dangerous than it is today,
because both add value to a slash path whose only trustless trigger can be
switched off with a deposit.

**Tests.** The harness test above inverts on v2: an unrelated deposit must leave
the fraud proof upholding. Plus per-certificate accounting: two certificates on
one vault, funding one must not back the other; withdrawal against one must not
draw down the other's reserve.

### What the trustless proof is, after R1 and R2

The per-certificate refactor restored `InsufficientReserve`. The adversarial
review then showed that the restored proof was doing two jobs it could not do,
and both have been taken off it. What is left is narrower than this section
implies, and the difference belongs here rather than only in §10.

**R1 — it may no longer be filed after the certificate's settlement deadline.**
`reserve_shortfall` compares the certificate's **immutable** claimed reserve
against the vault's **live** balance, and `release_to_operator` zeroes that
balance at `expires_at + CHALLENGE_WINDOW`. So at the exact instant the protocol
invites the operator to reclaim, every honestly completed certificate acquired a
permanently true proof — free to file, for anybody, with no fraud anywhere in
the story. Filing it re-froze the certificate, which trapped the auditor's
allocation (`release_allocation` is a separate call the auditor makes
themselves, unlocking at the same timestamp, with no atomic unwind and no
ordering requirement), and 72 hours later sent the whole allocation to the
treasury. Nobody was compensated and nobody profited: pure destruction of
auditor capital, priced at gas.

The rail is the deadline itself. `Registry::get_cert_settlement_deadline` is
already documented as "the instant after which nothing can still be proven
against this certificate, and therefore the instant its collateral may unwind";
both money contracts honoured the second half and nothing honoured the first.
`challenge` now refuses **any** filing from that instant onwards.

- **All proof types, not just this one.** The deadline's meaning is not
  proof-type-specific, and a `FakeSignature` or hygiene claim filed after it
  freezes and slashes exactly the same already-released collateral.
  Special-casing the predicate would leave the free freeze available through the
  others.
- **An already-open window is unaffected, by construction.**
  `get_cert_settlement_deadline` returns the **later** of
  `expires_at + CHALLENGE_WINDOW` and the freeze the ChallengeManager writes
  when a window opens. While a window is open the deadline **is** its
  `closes_at`, so joining claims pass the check and the window runs to
  completion and settles normally.
- **Nothing legitimately empties the reserve before the deadline.** §4 requires
  the money to be there at attestation; `release_to_operator` is locked until
  the deadline (the later of its deposit-time snapshot and the live one);
  `pay_from_reserve` is ChallengeManager-only and runs only inside a settlement
  that kills the certificate anyway. So for an attested certificate the balance
  is monotone until the deadline, and the rail cannot swallow a real claim.
  `an_attested_reserve_cannot_fall_short_before_the_settlement_deadline` pins
  this down on every ledger it matters on.
- **The cost: a genuine breach discovered after the deadline is
  unchallengeable.** That is accepted, because bounding exactly this is what the
  deadline is _for_ — the alternative is collateral that can never safely
  unwind — and because the alternative reading leaves every honest certificate
  permanently attackable by anyone.

**R2 — it pays no victim.** See §1. A predicate establishes that the covenant
broke, not that a named address lost anything.

**Where that leaves the proof.** `InsufficientReserve` is still trustless and
still unforgeable, and its remaining job is real: it kills an under-funded
certificate before or during its life, draws the operator's uncommitted money to
the treasury, slashes the auditor who vouched for it, and pays the challenger a
bounty for surfacing it. What it no longer does is compensate anybody or reach
past the deadline. Combined with §4, the honest reading is that **a trustless
proof can no longer, on its own, move money to a person** — that requires the
arbiter. This is a narrowing and it is stated rather than hidden.

**Tests** (section 14):
`a_lawful_reserve_withdrawal_manufactures_a_free_total_slash_of_the_allocation`
(the review's PoC, converted — the filing is refused and everybody ends whole)
and `a_window_opened_before_the_deadline_still_admits_claims_and_settles`.

---

## 10. Per-certificate stake allocation and one uniform settlement waterfall

**Status: implemented, not deployed** (branch `feat/settlement-waterfall`).

**Problem.** Every trustless proof in this protocol proves something _the
operator controls_: the reserve balance, the agent that spends, the expiry. So
the security question is never "can the proof be forged" — the arithmetic is
sound — but **"does manufacturing a true proof pay?"** Under v1 it paid
extremely well. `settle_fraud` slashed the auditor's entire live stake, sent 80%
of it to an address the challenger named and 20% to the challenger, then drained
the certificate's reserve to the same named address. A colluding operator named
itself and walked off with the auditor's whole bond.

### F02 — allocation, not a global stake

`AuditorStaking` no longer holds one global stake that a slash consumes whole.

- `Stake(auditor)` is total custodied capital; `Allocated(auditor)` is the sum of
  live allocations; **free = Stake − Allocated**.
- `Allocation(cert_id)` is the slice standing behind exactly one certificate,
  keyed the way the ReserveVault keys reserves.
- `is_registered` is judged on **free** stake. Under the old model one $500 stake
  could back an unlimited number of certificates at $500 of advertised
  collateral each.
- `release(auditor)` withdraws free stake only. Allocated capital is locked
  because a live certificate stands on it, not because of a timestamp.

**How the allocation amount is chosen.** It is a parameter on `attest`:
`attest(auditor, cert_id, allocation)`. The alternatives are both worse —
allocating the auditor's whole stake reproduces v1, where one bad certificate
destroys an entire book; a protocol-fixed amount prices every certificate the
same regardless of the bound it backs. The auditor is the party pricing the risk,
so the auditor names the number, and `AuditorStaking::allocate` enforces the two
limits that matter: at least `min_stake` (the same floor `is_registered` uses,
now applied per certificate) and no more than free stake.
`Certificate.auditor_stake_snapshot` becomes the allocation, so a counterparty
reading `verify` sees collateral that would actually be drawn on.

**How it retires.** At settlement the ChallengeManager calls
`retire_allocation(cert_id)`; on a clean expiry the auditor calls
`release_allocation(cert_id)` once the challenge window has closed. Either way
the unslashed remainder returns to **free stake**. Without that, capital is
stranded on dead certificates — the exact defect the refactor exists to remove.

### The waterfall

`settle_fraud` is one rule, applied identically to every proof type. Since §1 it
runs **once per claim window**, over the admitted set, and every pot in it is
divided pro rata by each claim's share of `total_harm`:

```
total_harm = Σ admitted claim weights   (see §1 on how weights aggregate)
payable    = min(total_harm, reserve_for_this_cert + allocation_for_this_cert)

reserve_draw   = min(payable, reserve)            <- all of step 1, unchanged
arbitrated_harm = Σ weights of ARBITER-ASSESSED admitted claims

1. reserve draw         <- the operator's own reserve for THIS certificate only
                           of which min(reserve_draw, arbitrated_harm) is paid
                           to the ASSESSED victims pro rata by assessed harm;
                           the rest -> TREASURY  (§1, R2)
2. challenger fee       <- the same reserve, 10% of PROVEN HARM
3. auditor slash        -> the TREASURY, capped by harm and by allocation
4. forfeited premium    -> the assessed victims, capped by ASSESSED harm the
                           reserve did not cover; the remainder + the unaccrued
                           share -> TREASURY
5. allocation retires; unslashed remainder returns to free stake
6. certificate invalidated; challenger's bond returned
```

| Rule                                                                     | Attack it closes                                                                                                                                                                                                                                                                                                                                                                                       |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Victim paid **only** from the operator's own reserve                     | A colluding operator paying its own colluder moves money from its left pocket to its right. Manufacturing a proof against yourself extracts nothing, so victim naming can stay as permissive as it is. Since R2 it is stronger still: a self-dealer's predicate claim is paid nothing at all, so the reserve draw leaves for the treasury and the pair ends up **down** by the harm they manufactured. |
| Victim paid **only** on ARBITER-ASSESSED harm (R2)                       | A predicate proves the covenant broke, not that a named address lost anything, so `n` sybil claims can no longer dilute an honest victim. The cost: the trustless proof compensates nobody directly. See §1.                                                                                                                                                                                           |
| Slashed stake goes **only** to the treasury                              | Removes the prize. Nobody who can trigger a proof can receive the auditor's money.                                                                                                                                                                                                                                                                                                                     |
| Challenger fee is a % of **proven harm**, from the reserve               | v1's 20%-of-stake fee made hunting _auditors_ profitable rather than hunting _fraud_.                                                                                                                                                                                                                                                                                                                  |
| Slash capped by allocation **and** by harm                               | A manufactured $10 breach cannot cost an auditor a $50,000 bond, and one bad certificate cannot destroy a book.                                                                                                                                                                                                                                                                                        |
| Unslashed remainder returns to free stake                                | Otherwise auditor capital is stranded.                                                                                                                                                                                                                                                                                                                                                                 |
| Forfeited premium to the victim is capped by uncovered **assessed** harm | Otherwise a large premium could pay a victim more than the harm proven against the certificate, breaking the "capped by proven harm" rail. Assessed rather than total since R2, so a predicate claim cannot enlarge the pot named victims share; with no assessed claim in the window the cap is zero and the whole forfeited premium goes to the treasury.                                            |

`harm` for `InsufficientReserve` is the shortfall, claimed reserve minus actual.
The named victim is deliberately not part of it: a named victim is a _filter, not
a proof_, and receiving a payment is evidence of being paid, not of being harmed.
Since R2 the named victim on a predicate claim is not part of the **payout**
either — the draw goes to the treasury. See §1.

**The arbiter states the quantity as well as the verdict.**
`resolve_by_arbiter(challenge_id, upheld, harm)` feeds its `harm` into the same
waterfall, so `BoundExceeded` and `FakeSignature` slash exactly like an
arithmetic proof does. This grants no new trust: for these proof types the
arbiter already decides whether fraud occurred at all, and their number is bound
by identical rails — `payable = min(harm, reserve + allocation)`, victim
compensation only from the operator's own reserve, slash only to the treasury. An
arbiter who overstates harm therefore cannot direct money to anyone who could
have bribed them; the self-dealing property is preserved by the waterfall, not by
the predicate. A negative `harm` is rejected, and `upheld == false` requires
`harm == 0`, so a contradictory call fails loudly rather than being silently
ignored. The alternative — a verdict with no quantity — meant every arbiter proof
settled in hygiene mode, letting an auditor who vouched for a certificate whose
agent blew through its bound walk away whole purely because the proof happened to
be arbiter-gated rather than arithmetic.

**Hygiene mode.** When `harm` is zero — computed or stated — the proof is real
but nobody can
evidence loss. The certificate is invalidated, the allocation retires in full,
the reserve is not touched, and the challenger is paid a **flat** $10 bounty out
of forfeited bonds — the only pot available, since the reserve is off limits by
definition and paying from the stake would reinstate rule 3's prize. An empty
pool pays nothing.

**Treasury.** Named once at `initialize`, with no admin and no upgrade path.
Making it mutable would reopen the prize.

### The premium economy — step 4, and the PremiumVault

**Status: implemented, not deployed** (branch `feat/premium-economy`).

Step 4 was a comment. It is now a contract.

`contracts/premium-vault` is a **new, per-certificate** contract in the
ReserveVault's storage style (`Coverage(cert_id)`). It is deliberately _not_ an
extension of `fee-escrow`: that contract is a singleton whose `Released` flag
never resets, so it pays out exactly once ever, and the ChallengeManager never
calls it. That is known defect **L3**, and building a premium economy on top of
it would inherit the defect rather than fix it. `fee-escrow` is untouched and
stays off the settlement path.

**`fee-escrow` should now be deleted rather than merely bypassed.** Nothing
depends on it, and dead code that holds funds is worse than no code — especially
into a redeploy with no upgrade path, where anything shipped can only be removed
by another redeploy. The deletion is blocked on TypeScript, not Rust:
`packages/sdk/src/deployments.ts` declares `contracts.feeEscrow` as a required
field, so `scripts/deploy-all.ts` cannot stop producing an address until that
field becomes optional. See `docs/V2-CUTOVER.md` for the exact sequence. Until
it lands, **L3 stays open**.

#### Pricing

```
premium = bound * rate_bps * duration_seconds / (10_000 * SECONDS_PER_YEAR)
SECONDS_PER_YEAR = 31_536_000
```

`rate_bps` and `fee_bps` are **simple, transparent, configurable parameters set
at `initialize`** — no actuarial model, no risk tiering, no external
underwriter, all of which this milestone explicitly excludes. Risk-based pricing
needs a loss history the protocol does not have, and inventing one in code would
be a lie dressed as a model. There is no admin, so re-pricing means a fresh
deployment. The deploy script ships 200 bps and a 10% fee share.

Worked example, and the test that pins it: a **$1,500 bound at 200 bps for 90
days** is `15_000_000_000 * 200 * 7_776_000 / (10_000 * 31_536_000)` =
**73_972_602 stroops** ($7.3972602). The exact rational value carries a
`.7397…` tail; integer division truncates it.

**Truncation is a chosen direction, not an accident.** Every division truncates
toward zero: the operator is charged **no more** than the exact price, and the
auditor accrues **no more** than the exact figure, so the vault can never owe
out more than it holds. Each error is at most one stroop (10⁻⁷ USDC). One real
consequence, asserted rather than hidden: **linearity is not exact at arbitrary
magnitudes.** Doubling the duration of the 90-day case gives 147_945_205, which
is `2 × 73_972_602 + 1` — two truncated halves lose more than one truncated
whole. The linearity test uses periods that divide `SECONDS_PER_YEAR` evenly, so
the identity it asserts really is one, and a separate test pins the off-by-one
where it bites.

**Which duration: `expires_at - issued_at`, not `expires_at - now`.** Both are
immutable fields of the certificate. Pricing from `now` would make the premium a
function of _when the operator chose to call `pay_premium`_, and an operator
would simply wait until the instant before expiry and buy a year of coverage for
a day's price. Anchoring to `issued_at` removes the choice: the price is fixed at
publish, and no transaction timing can move it.

A zero-bound or zero-duration certificate prices at zero and is recorded as paid
without moving a stroop. Neither is reachable through the Registry, which rejects
both at `publish`, but the vault does not panic on them.

#### Accrual and claiming

The premium accrues to the certificate's auditor **straight-line over the
coverage period**: `accrued = yield_pot * elapsed / duration`, clamped to
`[0, yield_pot]`. It is yield on **staked** capital, so the certificate must be
attested before coverage can be bought — there has to be an allocation for the
yield to be yield _on_.

**Claiming is allowed at any time, including mid-coverage.** Straight-line
accrual makes that the natural reading: at every instant the accrued figure is
precisely payment for coverage already delivered, and making the auditor wait
until expiry would be an interest-free loan from the auditor to the protocol for
no security gain. It is safe _because_ forfeiture takes only **unclaimed** yield,
so an auditor who claims continuously is converting forfeitable yield into
settled income as fast as they earn it.

The honest cost, stated rather than glossed: **a diligent auditor who claims
often forfeits almost nothing on a slash.** That is accepted, because the
auditor's skin in the game is their **allocation**, which stays fully slashable
however fast they claim. The premium is yield on that capital, not a second bond.
Treating unclaimed premium as collateral would over-state the protocol's teeth.

#### The protocol fee share

`fee_bps` of each premium is transferred to the treasury **at payment time**,
not held and released later. Holding it would create a second pot with its own
release rules and its own way to get stuck — which is exactly what `fee-escrow`
demonstrates. Nothing about the fee is contingent, so nothing needs deciding
later. The auditor's pot is `premium - protocol_fee` and there is no path by
which the auditor can reach the fee.

#### Forfeiture, and step 4

When an auditor is slashed for a certificate, they forfeit **unclaimed** yield on
it. `PremiumVault::forfeit(cert_id, victim, victim_cap)` splits the pot:

- **accrued-but-unclaimed → the victim**, capped by `victim_cap`;

Since §1 the ChallengeManager passes **itself** as the recipient and fans the
money out to the window's victims pro rata inside the same invocation. `forfeit`
pays one address and a window can admit many victims; calling it once per victim
would hand the whole pot to whoever was called first, which is exactly the
ordering value §1 exists to remove. The ChallengeManager is a conduit for the
length of one call — nothing observes the intermediate balance and `bonds_held`
is untouched, so the hygiene bounty pool is unaffected.

- **the excess over the cap, plus the entire unaccrued remainder → the
  treasury**.

`victim_cap` is `harm - victim_amount`: the harm the operator's own reserve did
not already cover.

**Why this does not break rule 1.** Victim compensation still comes only from the
operator's own money, because **the premium _is_ the operator's money** — every
stroop in that pot was paid in by this certificate's operator and has not yet
been handed to anyone else. Paying a victim from it is the same
left-pocket-to-right move that makes a self-dealing operator's "compensation" a
wash.

**Why this does not break rule 3.** The vault has no reference to
`AuditorStaking` and moves only tokens it already holds. Nothing a challenger or
victim can trigger can pay them the auditor's stake.

**Why this does not break "capped by proven harm."** That is what `victim_cap`
is for. Without it a large premium could pay a victim more than the harm proven
against the certificate.

**Already-claimed yield is not clawed back, and no clawback is attempted.** The
money has left the contract. Writing a clawback that cannot work would be a lie
in the code.

#### Does forfeited premium raise `payable`? No — and the reason is arithmetic

It was considered and rejected, because **it is provably a no-op.** `payable`
binds through exactly two mins: `victim_amount = min(payable, reserve)` and
`slash = min(payable - victim_amount, allocation)`. Adding a premium `P` gives
`payable' = min(harm, reserve + allocation + P)`.

- If `harm < reserve + allocation`, then `payable' = payable = harm`. Nothing
  changes.
- Otherwise `victim_amount` is `reserve` either way (since `payable ≥ reserve`),
  and `slash` is capped at `allocation` either way (since
  `payable' - reserve ≥ allocation`).

So folding `P` into `payable` changes neither line — while making steps 1 and 3
_read_ as though the premium could enlarge them. Keeping `payable` as the cap on
**collateral** (reserve + allocation) and giving the premium its own explicit cap
in step 4 is the same money with none of the ambiguity. It also honours what the
original gap note promised: step 4 arrives without disturbing the amounts above
it.

The attack this closes is a readability one, which is the honest way to put it:
there is no arithmetic attack either way, and the risk being managed is a future
reader mis-reading the cap and "simplifying" one of the two mins away.

#### Hygiene mode (`harm == 0`)

The proof is true, nobody is evidenced as harmed and **the auditor is not
slashed**, so nothing is forfeited to a victim.
`PremiumVault::terminate(cert_id)` instead freezes accrual at the kill: the
auditor **keeps** — and can still claim — the share they earned up to that
instant, and the unaccrued remainder goes to the treasury.

The remainder is deliberately **not** refunded to the operator. §10 already
prices a hygiene kill as costing the operator their certificate, their reserve
lockup _and their premium_: both hygiene predicates are manufacturable by the
operator for the price of gas, and a refund would make manufacturing one free.

#### Wiring

`ChallengeManager::set_premium_vault(vault)` — one-shot, arbiter-gated, exactly
like `set_router` and for the same reason: whoever names the vault names the
contract that is handed the forfeited premium and told where to send it. It
cannot be re-pointed.

**Same trap as `set_router`, and it is worth stating twice.** If the vault is
never set, step 4 is _silently skipped_ on every challenge — the vault still
takes operators' money and never forfeits it — while every contract test passes.
It is a skip rather than a panic so that a deployment predating the premium
economy can still settle. `ChallengeManager::has_premium_vault()` exists so a
deploy check can catch it, and `scripts/deploy-all.ts` makes the call with a
comment saying why.

### The post-expiry timing bug

The reserve used to unlock at `expires_at`, and so did the auditor's stake. But
an `ExpiredCertificate` proof is about activity _after_ expiry, so by the time it
became provable the reserve was withdrawn and the stake was free: it would settle
against an empty pot every time. Both now lock until
`expires_at + CHALLENGE_WINDOW_SECONDS` (7 days), a constant owned by the
Registry and read by both the vault and the staking contract through
`get_cert_settlement_deadline(cert_id)`, so the two can never drift apart.

### The two trustless predicates, and why they never slash

**Status: implemented** (branch `feat/trustless-predicates`).

`BoundExceeded` and `ExpiredCertificate` are now proven by `resolve` from
on-chain state, with the PaymentRouter as the source of truth. Neither needs the
arbiter any more.

- **`BoundExceeded`** — `router.spent(cert_id) > certificate.bound`.
- **`ExpiredCertificate`** — §7 in full, applied to the router's `PostExpiry`
  record: the largest post-expiry payment settled after
  `expires_at + GRACE_WINDOW_SECONDS` (24 hours), was at least
  `DE_MINIMIS_FLOOR_BPS` of the certificate's own bound (0.1%), and the
  certificate is neither invalid nor superseded by a newer certificate for the
  same agent.

The router is a sound source for both: only an enrolled agent moves the counter,
enrollment needs the agent's **and** the certificate operator's signature, and an
enrollment is permanent. So nobody can attach spend to a stranger's certificate,
and no operator can walk an agent off a climbing counter onto a fresh one. An
agent that never enrolled meters nothing, so neither predicate can be true
against its certificate at all.

**Both settle in hygiene mode. The auditor is not slashed.** This is the
security property, not an unfinished job, and the reasoning is repeated in the
source because a future reader will otherwise "fix" it back into the
vulnerability:

Both predicates are **manufacturable at will by the operator**. `spend-probe` and
the router's own shuttle test prove it for `BoundExceeded` — a $1 shuttle between
two addresses the operator controls drives `spent` past any bound for the price
of gas, with net flow of exactly zero. `ExpiredCertificate` is the same shape:
the operator controls whether their own agent keeps paying after expiry. If
either slashed on the counter alone, **any operator could destroy their auditor's
allocation for the cost of gas**. The slash goes to the treasury, so the operator
gains nothing — but the auditor loses everything, at will, with no defence. An
auditing business cannot exist under that rule, and the protocol is worthless
without auditors.

What these proofs establish is that the covenant was broken, not that anyone was
harmed. The certificate dying is the correct and sufficient automatic
consequence: it costs the operator their own certificate, their reserve lockup
and (later) their premium, and it warns every counterparty. Compensation and
slashing require assessed harm, which is what
`resolve_by_arbiter(challenge_id, fraud_proven, harm)` is for — and it still
slashes the identical breach when a human states a number.
`InsufficientReserve` keeps the full waterfall unchanged: its harm is a shortfall
in capital the operator promised and did not commit, which cannot be
manufactured without actually losing that capital.

**Since R4, that human may only ever raise the number, never lower it, and may
not touch the verdict at all.** A predicate's finding is the contract's own, and
nothing about "the counter is evidence, not a loss" requires the arbiter to be
able to say the counter is wrong. Stating a harm on top of a true predicate is
the whole of the trust being granted here; overturning the predicate was never
part of it and is now refused.

**Wiring.** The ChallengeManager learns the router's address through
`set_router`, a one-shot call authorized by the arbiter, rather than a ninth
`initialize` argument that would break the committed bindings and the deploy
script positionally. It is arbiter-gated because whoever names the router names
the contract that reports `spent`, and a lying router could invalidate any
certificate it liked. It cannot be re-pointed, for the same reason the treasury
cannot.

### The griefing residue the waterfall did **not** close

Stated plainly, because the waterfall is easy to over-claim.

The waterfall makes manufacturing a proof **unprofitable** — a self-dealing
operator who names their own address as victim moves money from their left
pocket to their right, and the slash goes to a treasury they do not control. It
does **not** make `InsufficientReserve` **costless to the auditor**.

An operator who deliberately under-funds their own certificate and then
challenges it destroys their auditor's allocation, which goes to the treasury,
for the price of gas plus their own reserve and their own certificate. The
`self_dealing_against_an_empty_reserve_hands_the_colluders_nothing` test
demonstrates exactly this and is named for only half of what it shows: the
colluders gain nothing **and the auditor's whole allocation goes to the
treasury**. The attacker is not enriched; the auditor is still ruined.

The reserve check at `attest` (§4) **is now implemented**, and it closes more of
this than the paragraph above anticipated. An auditor cannot be walked into a
certificate that is already fraudulent; and because `deposit` locks the reserve
until the certificate's settlement deadline, the operator cannot legitimately
withdraw it afterwards either, until that deadline passes. The cheap version of
this attack — publish empty, get attested, self-challenge the same day — is gone.

What remained was described here as "the expensive version": post the full
reserve, leave it locked for the certificate's whole life plus the challenge
window, then withdraw it at the deadline and self-challenge against an auditor
who has not yet reclaimed their allocation.

**That paragraph was wrong about the price, and the adversarial review's R1 is
why.** The withdrawal at the deadline is not a sacrifice — it is the operator
getting all of their money back, exactly as the protocol invites. There was no
under-funding, no lost reserve, and the certificate being killed had already
expired and was worthless. And the attacker need not be the operator at all:
the proof was true for anybody the moment the vault emptied. So this was not an
expensive griefing residue but a **free, total slash available to any address
against every honestly completed certificate**.

**Fixed** (`f2de15b`): `challenge` refuses any filing from the certificate's
settlement deadline onwards. See §9's "What the trustless proof is, after R1 and
R2" for the rail, why it covers all proof types, why an already-open window is
unaffected, and what it costs. The test that used to keep this residue honest,
`a_reserve_withdrawn_after_attestation_is_still_provable_fraud`, has been
converted into
`an_attested_reserve_cannot_fall_short_before_the_settlement_deadline`, which
asserts the opposite and shows why the opposite is safe.

**What is left of the residue.** The cheap version was closed by §4 and the
expensive version turns out not to have existed, because the vault lock means an
attested certificate's reserve is monotone until its deadline. An operator who
wants their auditor slashed must therefore under-fund **before** attestation —
which §4 refuses — or persuade the arbiter. The candidate directions listed here
before (barring the operator from challenging their own certificate; routing a
self-challenge's slash to the auditor; treating an operator-initiated proof as
hygiene) are no longer needed for this and are dropped.

### The claim window's own residue

**Status: implemented, not deployed** (branch `feat/claim-window`). See §1 and
§2 for what was built. What it did **not** close:

- **A cure is free, and that under-prices getting caught.** §2 states the
  decision and its reasoning. An operator can run a persistently underfunded
  certificate and top it up only when challenged, using the 72 hours as free
  credit. The obvious next step — a per-certificate cure counter with a fee or a
  forced re-attestation on repeat — is not built, because pricing it without a
  loss history would be inventing a number.
- **An arbiter-gated claim freezes a certificate on a minimum bond — now for as
  long as the arbiter takes, not a fixed 72 hours.** A `FakeSignature` claim has
  no on-chain predicate, so it cannot be rejected at filing the way a false
  `InsufficientReserve` claim is; it opens a window and waits for the arbiter.
  That was a griefing surface priced at one minimum bond for three days of
  frozen reserve and allocation, and an arbiter ruling the claim false did
  nothing to shorten it.

  **Chosen fix: `close_window_early(challenge_id)`.** An arbiter rejection ends
  the window on the spot. Arbiter-only, and only against a claim the arbiter
  themselves ruled false (`arbitrated && adjudicated && !proven`) — a claim a
  _predicate_ found false is not a key to this door, because nobody exercised
  judgement on it.

  **The rail: it refuses while any other live claim remains.** A live claim is
  any other claim in the window that could still be admitted at close — one the
  arbiter has **not yet ruled on** (they may still uphold it), or one that is
  **proven**, whether an arbiter stated it or a predicate computed it. A proven
  claim counts as live _even if the operator has since cured the underlying
  condition_: the cure check belongs at close and only at close, or an operator
  could cure mid-window, have a throwaway claim rejected, and cut the window out
  from under a genuine victim who had not yet been paid. The conservative
  reading is the correct one, and it costs nothing — the only claims that fail
  to hold a window open are the ones already known to be wrong at filing, which
  would be rejected and paid nothing anyway. So the early close can only fire
  when settling right now is _guaranteed_ to admit nothing, which makes it
  indistinguishable in outcome from the close that was coming anyway.

  **Getting rejected faster is not cheaper.** The early path runs through the
  same internal `settle_window` a natural close uses, so the griefer's bond is
  forfeited exactly as it would have been at `closes_at`. No discount.

  **That was not enough, and review R3 is why.** `close_window_early` needs the
  arbiter to actually rule, so it is unavailable in exactly the scenario that
  defines the attack — an arbiter who says nothing. Worse, this section priced
  the surface at "one minimum bond", and the code never charged it: a claim
  nobody ruled on resolves `Unadjudicated`, which returns the bond **in full**.
  The freeze cost gas and 72 hours of float, repeatable in 72-hour blocks.

  **Fixed (`3c5a46d`) by the third mitigation listed here as unchosen:
  arbiter-gated claims do not freeze at all.** The freeze is no longer held by
  the **window**; it is held by a claim something actually backs — a predicate
  that evaluated true at filing, whether it opened the window or joined one, or
  the arbiter **upholding** a `FakeSignature` claim. A claim nobody has ruled on
  locks no reserve and no allocation, so the grief has nothing to buy at any
  price, and the 72 hours cost the operator and the auditor nothing to wait out.

  **Why not the other candidate — a higher bond for arbiter-gated proof types.**
  It prices the grief rather than removing it, and an attacker who can afford the
  price still gets the freeze. On this attack the lever does not even connect:
  the bond is refunded, so raising it raises the griefer's float and not their
  cost. Making it bite would mean forfeiting the bond of a claimant whose arbiter
  never ruled — punishing the innocent for the arbiter's silence, which is the
  one outcome `Unadjudicated` exists to prevent.

  **`Unadjudicated` still refunds in full, now as a decision rather than a
  leak.** Its stated reason was always right; it was a problem only because the
  claim bought a freeze on the way past. With the freeze gone the refund costs
  the protocol nothing.

  Residue: an upheld `FakeSignature` finding can settle against collateral that
  has already unwound. The exposure is bounded rather than open-ended — the
  reserve and the allocation are locked until the certificate's own settlement
  deadline whatever the ChallengeManager does, and R1 already refuses any filing
  from that deadline onwards, so the loss requires the arbiter to still be silent
  at a deadline the claim was necessarily filed before. That is a latency
  controlled by a party already trusted completely with the verdict itself.
  `close_window_early` is kept: ending a rejected claim's window is still the
  right outcome, it is simply no longer load-bearing.

- **Nobody is paid to close a window.** `close_window` is permissionless and
  unrewarded. The claimants' own money is the incentive, which is sound but is
  not a guarantee: a window with only a hygiene claim in it has almost nothing
  behind it, and the certificate stays frozen until somebody spends the fee.
- **72 hours is a proposal.** It is long enough to be a real cost to a victim
  and short enough that a claimant in a bad timezone can miss it. It is not a
  researched number.
- **The predicate group's equal split can under-pay a large real victim.** Two
  `InsufficientReserve` claimants share the shortfall equally, because the
  predicate cannot tell them apart. If one genuinely lost far more than the
  other, only the arbiter path can express that — and the arbiter path is
  trusted. This is the honest limit of a certificate-level predicate. **R2 took
  this to its conclusion**: the predicate group is no longer paid compensation
  at all, and the arbiter path is the only one that is. See §1.
- **Sybil claims still split the challenger FEE, and that is deliberate.** R2
  removed them from the victim pot, not from the fee, and the line is drawn
  there on purpose. The fee is a **bounty for surfacing a fact**, not
  compensation for a loss; its total is fixed at 10% of proven harm however many
  people file, so a sybil cannot enlarge it — they can only take a share of one
  honest challenger's bounty. That is the same trade the hygiene bounty already
  makes on purpose ("one flat bounty for the window, split equally, not one
  bounty each"). Removing it would need an identity the predicate does not have,
  and the exposure is bounded at 10% of harm rather than 100% of it.
- **R2 widened the arbiter's power, and therefore R4's — so R4 was fixed**
  (`3c5a46d`). Victim compensation flows only through `resolve_by_arbiter`, and
  `resolve_by_arbiter` could overturn a true `InsufficientReserve` claim
  outright, which meant one party could both withhold compensation and veto the
  arithmetic that would have slashed. It now may **add** to what an on-chain
  predicate proved and may never **contradict** it: on any proof type but
  `FakeSignature`, the verdict must match what the predicate recorded at filing
  and the harm may not fall below the number it computed.

  A ratchet rather than a ban, because a ban would delete the "a human states a
  number" path two paragraphs up — the only way a real loss behind a
  `BoundExceeded` or `ExpiredCertificate` counter reaches the waterfall — and
  would leave the victim of a reserve shortfall with no correctly-labelled route
  to compensation at all. The harm **floor** is part of the same rule rather than
  an extra: leaving the verdict alone and writing $1 over a $1,000 shortfall
  drops the claim out of the predicate group and shrinks the slash to nothing, so
  a quantitative veto is the same veto. Raising the number stays open, because a
  shortfall is a floor on what was lost and not a ceiling.

  It also closes a direction nobody had noticed: a predicate claim that was
  **false** at filing and **joined** an open window was stored `proven = false`
  rather than rejected on the spot, and the arbiter could flip it true. The
  equality rule refuses that too, which is what finally makes
  `resolve_by_arbiter`'s long-standing claim — "no human may declare a breach
  they say did not happen" — actually true.

  What is unchanged: the arbiter is still the only route to victim compensation,
  and their number is still unbounded above and unappealable. R4 was never about
  the size of that trust, only about it reaching further than the docs claimed.

- **An arbiter who overstates harm now dilutes the honest claimants sharing the
  window,** as well as enlarging the slash. The waterfall's rails still hold —
  nothing reaches anyone who could have bribed them — but the dilution is a new
  cost that did not exist before aggregation.

### Gaps deliberately left open

- **The renewal cure path is now real machinery rather than an accident.** §2's
  evaluate-at-filing exists, so an `ExpiredCertificate` claim that a renewal
  defeats mid-window resolves `Cured` with the bond returned, instead of
  silently failing. The coarseness noted below is unchanged; its cost to the
  challenger is not.
- **`ExpiredCertificate` judges one payment, not a history.** The router records
  a single post-expiry pair — the largest late payment and its timestamp — so a
  large payment inside the grace window masks a smaller, later one that would
  have cleared both tests. Conservative in the safe direction (fewer upholds),
  and closing it would put unbounded storage on the router's x402 hot path.
- **The renewal check is coarser than §7's wording.** §7 says "not renewed
  _before_ the payment"; the predicate asks "not renewed as of resolution", so a
  renewal filed _after_ the late payment also defeats the proof. That is a cure
  path in the sense of §2 — and now that §2's machinery exists, it resolves as
  `Cured` with the bond returned rather than costing the challenger anything.
  The timing is still coarser than §7's wording, and it still errs toward not
  killing a certificate.
- **The grace window and the floor are unresearched proposals.** 24 hours and
  0.1% are §7's numbers, adopted as-is. Both are attack surface: the window is
  free post-expiry coverage a hostile operator can plan around, and the floor is
  a band of payments that provably breach the covenant and are unprovable anyway.
- **A premium vault that is never wired is silently inert.** `set_premium_vault`
  is one-shot and arbiter-gated; skipping it makes step 4 a no-op on every
  challenge while the vault keeps accepting premiums. `has_premium_vault()` is
  the check; nothing enforces it.
- **Nothing forces an operator to buy coverage.** `pay_premium` is voluntary and
  not a precondition of `publish`, `attest` or `verify`, so a certificate can be
  fully valid with no premium behind it and step 4 settles as a no-op for it. The
  milestone asks for premiums priced and accrued, not for coverage to be
  compulsory; making it compulsory is a `publish`-time change and therefore a
  Certificate/bindings change.
- **A slashed auditor who claimed early loses almost nothing.** Continuous
  claiming is deliberate (see above) and the allocation is the real
  consequence — but the premium forfeiture is a weak deterrent by construction,
  not a strong one.
- **The premium address is not in `deployments/*.json` yet.** The contract map
  lives in `packages/sdk/src`, which this branch does not touch; the address is
  written to `.env.testnet` and the map gains its key in the cutover change that
  regenerates bindings.
- **`rate_bps` is one number for every certificate,** regardless of the auditor,
  the operator or the bound's composition. That is the milestone's explicit
  instruction, and it is also a real limitation: coverage is mispriced for
  everybody except the median risk.
- **The arbiter's harm figure is unbounded and unappealable.** It is capped by
  the certificate's collateral, but nothing checks it against evidence and there
  is no dispute path once it is stated. That is the same trust already placed in
  the verdict itself, now extended to an amount as well as a boolean.
- **Still first-resolver-takes-all** (§1). The waterfall settles one challenge
  immediately; the aggregating claim window is not built.
- **The hygiene bounty depends on forfeited bonds.** A protocol with no failed
  challenges yet pays nothing for hygiene work.
- **No admin, so no way to fix a mis-set treasury**, by design.

---

## What this changes downstream

- **Certificate struct** gains: float cap (§6), and whatever the claim window and
  tracking flags require (§1, §8). All at publish time — none retrofittable.
- **`attest`** becomes a cross-contract call (§4).
- **`publish`** gains vault-allowlist validation (§3), and **requires the agent's
  signature as well as the operator's** (defect L1). Two signatures, one
  transaction — Soroban permits one contract call per transaction, so a client
  must collect the agent's authorization entry rather than submitting twice.
- **Challenge lifecycle** grows from two states to four: `Open`, `Upheld`,
  `Rejected`, `Cured` (§2), with an aggregating claim window (§1).
- **A new admin/governance surface** with a timelock and a seal (§5).
- **`VerifyResult` / `CertView`** gain `tracked` and the float cap (§6, §8).

Four of these are storage-layout changes to the certificate itself, which is why
this is a redeploy and not a patch.

## Storage lifetime — defect L2

**Status: fixed across all eight contracts.**

Soroban archives instance and persistent entries once their TTL lapses, and
**reaching an archived entry aborts the transaction** rather than returning a
default. Nothing in the workspace extended any TTL, so two failures were waiting:
a certificate nobody touched would stop being readable, and each contract
_instance_ is on the same clock — an archived instance takes the whole contract
offline, not one entry. The registry's unit tests demonstrate both halves against
the real host rather than asserting them in prose.

Every write path now bumps the instance entry, and every persistent entry a
contract creates is extended. Two named constants, defined per contract with the
full reasoning on the registry's copy:

| Constant        | Value                | Why                                                                                                                                                              |
| --------------- | -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TTL_EXTEND_TO` | 120 days (2,073,600) | Chosen against the lifetime of the thing protected: a certificate's `expires_at` plus the 7-day challenge window. Every entry a certificate creates outlives it. |
| `TTL_THRESHOLD` | 60 days (1,036,800)  | Half the runway. `extend_ttl` is a no-op above the threshold, so this is at most one rent payment per 60 days of activity rather than one per call.              |

Both sit under the 3,110,400-ledger `max_entry_ttl` the live networks configure,
so an extension is never clamped.

**Who pays, stated plainly.** TTL extension is rent, charged to the submitter of
the transaction that triggers it. Every bump sits on a write path, so the payer
is the operator, agent, auditor, arbiter or challenger already paying for that
call. The protocol never pays.

**The deliberate residual.** No read-only path was turned into a state change, so
a certificate that nobody transacts against at all for 120 days still archives.
Making `verify` bump TTLs would fix that and would also make a counterparty's
read a fee-bearing write — which is the wrong trade for the protocol's most-used
call. Any transaction against the certificate resets the clock.

**SEP-41 allowances are deliberately excluded.** They live in the router's
temporary storage with an expiry the approver names. Extending them past that
would be the contract overriding the approver; letting them archive fails closed,
to an allowance of zero.

**`extend_ttl` on a missing key is a host error, not a no-op.** `auditor-staking`
guards the entries that do not exist until an auditor first allocates.

## Still genuinely open

- §3: whether removing a vault hash from the allowlist invalidates existing
  certificates. Leaning no; must be decided before implementation. Note that §4
  no longer waits on §3 — it reads the ChallengeManager's vault rather than the
  operator-supplied one — but `cert.reserve_vault_contract` is still an
  unconstrained, operator-written field that nothing reads. Either §3 gives it
  meaning or it should be removed from the struct.
- Every numeric parameter above is a **proposal**, not a decision: the 72-hour
  claim window, the 7-day timelock, the 24-hour grace window, the 0.1% floor.
  They need to be argued individually, and each one is an attack surface.
- Whether `Cured` should cost the operator anything at all. Currently free, which
  may under-price getting caught.
- **A metered transfer emits two events, not one.** The router emits the standard
  `transfer` event plus a `spend` event for indexers. The x402 constraint is
  usually stated as "exactly one transfer event", and a facilitator matching on
  the transfer topic is satisfied — but a facilitator that requires exactly one
  event _in total_ would reject metered payments. This is untested against a real
  facilitator and must be confirmed before the router settles live x402 traffic.
  If it fails, the `spend` event has to move out of the transfer path.
- **No clawback from a halted certificate.** Halt gates `withdraw` too, so an
  honest operator cannot recover their own float while halted, and resuming to
  recover it re-enables the thief. See `docs/THREAT-MODEL.md`; this is the most
  important open item there.
- **The float cap lives on the router, not the certificate.** § 6 calls for it to
  be set at publish and visible in `get_certificate` and `CertView`. That is a
  `Certificate` struct change and therefore a bindings/SDK change, so it was left
  out of the Rust-only build. Until it moves, a counterparty cannot see the cap
  on the certificate — which was half the point of having one.
- **§ 8's `tracked` field is not on `VerifyResult`.** Same reason. `is_tracked()`
  exists on the router for a future `verify` to read.
