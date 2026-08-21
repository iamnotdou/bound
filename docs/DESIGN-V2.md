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

**Problem.** If one settlement extinguishes a certificate, a self-challenge for
the minimum bond permanently forecloses every honest claim behind it. The attack
costs the minimum bond and destroys the entire coverage.

**Decision. A claim window that aggregates before paying.**

A first valid challenge opens a fixed **claim window** (proposed: 72 hours of
ledger time) rather than settling. During the window any party may file an
additional claim against the same certificate. The certificate is frozen: no new
attestation, no reserve withdrawal, no expiry-based escape. At window close,
settlement runs **once**, over all admitted claims:

- Claims are paid **pro rata** from the available collateral when total proven
  harm exceeds it, in full when it does not.
- Pro rata, not first-come — ordering within a window must not be worth anything,
  or we have rebuilt the race we are removing.

**Rejected — multi-claim settlement without a window.** Pay each valid claim as
it resolves until collateral is exhausted. Simpler, but it keeps the race: the
attacker still front-runs honest claimants, they just drain rather than
extinguish. It makes the ordering profitable, which is the actual defect.

**Rejected — one certificate, one claim, ever.** This is v1. It is the bug.

**Cost, stated plainly.** A genuine victim now waits out the window before being
paid. That is the price of not letting a self-dealer foreclose them, and it
should be documented in the trust model as a known latency, not hidden.

**Tests.**

- Two claimants, collateral sufficient → both paid in full, in one settlement.
- Two claimants, collateral insufficient → paid pro rata; the sum paid equals
  available collateral exactly; no dust is stranded and none is minted.
- A minimum-bond self-challenge filed first → does **not** foreclose an honest
  claim filed later in the same window; the honest claimant's share is unchanged
  by the self-challenge's presence beyond its own pro-rata dilution.
- A claim filed one ledger after window close → rejected.
- Reserve withdrawal during an open window → rejected.

---

## 2. Curing a proof mid-challenge — CRITICAL

**Problem.** With a two-phase challenge, an operator can top up the reserve
between filing and resolution, flip the predicate to false, and pocket the
challenger's forfeited bond. The protocol pays the operator for having been
caught.

**Decision. The predicate is evaluated at filing, and a cure returns the bond.**

Two mechanisms, both required:

1. **Evaluate at filing.** The challenge records the predicate's inputs at the
   filing ledger. Resolution re-checks the recorded state, not live state. What
   was true when the challenge was filed stays true.
2. **A cure returns the bond.** If the operator remedies the condition during the
   window, the challenge resolves as `Cured`: the challenger's bond is returned
   **in full**, the certificate survives, and no slash occurs.

`Cured` is a third outcome alongside `Upheld` and `Rejected`. A challenger who
was right about the state at filing never loses their bond, even when the
operator fixes it. Only a challenger who was **wrong at filing** forfeits.

**Rejected — evaluate at filing only.** Sound against the theft, but it forces a
slash on an operator who fixed the problem within hours. That over-punishes and
discourages exactly the remediation we want.

**Rejected — bond returned on cure only.** Without filing-time evaluation the
operator still controls the predicate at resolution time, so it does not close
the hole; it only makes the theft cheaper.

**Note.** This interacts with §1: a cure must close the whole claim window, and
claims filed after the cure must be rejected rather than aggregated.

**Tests.**

- Reserve topped up after filing → resolves `Cured`, bond returned in full, no
  slash, certificate still valid.
- Reserve topped up after filing, on a challenge that was **false at filing** →
  resolves `Rejected`, bond forfeited. The cure does not launder a bad challenge.
- Live state manipulated between filing and resolution → recorded state governs;
  the outcome is identical to resolving at the filing ledger.
- A `Cured` resolution must leave the auditor's stake untouched.

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

`attest` performs the same reserve check `verify_insufficient_reserve` performs,
against the certificate's vault, and rejects if the vault does not hold at least
the claimed reserve. An auditor cannot sign a certificate that is already
fraudulent.

This is the one-line fix the review identified, and it changes `attest` from a
registry-local write into a cross-contract call. That is a real shape change —
`attest` now depends on the vault being live and on §3's allowlist — and it must
land in the same redeploy as both.

**Not sufficient on its own.** It closes attestation-time griefing. It does not
stop the operator withdrawing the reserve _after_ attestation, which is what the
`InsufficientReserve` proof and the auditor's own monitoring are for. The
auditor's risk is real and ongoing; this only removes the instant-loss trap.

**Tests.**

- `attest` on a fully-funded certificate → succeeds.
- `attest` on an underfunded certificate → rejected, no attestation recorded.
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

`settle_fraud` is one rule, applied identically to every proof type:

```
harm    = raw_harm_from_predicate
payable = min(harm, reserve_for_this_cert + allocation_for_this_cert)

1. victim compensation  <- the operator's own reserve for THIS certificate only
2. challenger fee       <- the same reserve, 10% of PROVEN HARM
3. auditor slash        -> the TREASURY, capped by harm and by allocation
4. (premium step — not built; documented gap)
5. allocation retires; unslashed remainder returns to free stake
6. certificate invalidated; challenger's bond returned
```

| Rule                                                       | Attack it closes                                                                                                                                                                                       |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Victim paid **only** from the operator's own reserve       | A colluding operator paying its own colluder moves money from its left pocket to its right. Manufacturing a proof against yourself extracts nothing, so victim naming can stay as permissive as it is. |
| Slashed stake goes **only** to the treasury                | Removes the prize. Nobody who can trigger a proof can receive the auditor's money.                                                                                                                     |
| Challenger fee is a % of **proven harm**, from the reserve | v1's 20%-of-stake fee made hunting _auditors_ profitable rather than hunting _fraud_.                                                                                                                  |
| Slash capped by allocation **and** by harm                 | A manufactured $10 breach cannot cost an auditor a $50,000 bond, and one bad certificate cannot destroy a book.                                                                                        |
| Unslashed remainder returns to free stake                  | Otherwise auditor capital is stranded.                                                                                                                                                                 |

`harm` for `InsufficientReserve` is the shortfall, claimed reserve minus actual.
The named victim is deliberately not part of it: a named victim is a _filter, not
a proof_, and receiving a payment is evidence of being paid, not of being harmed.

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

The minimum-reserve-funding check at `attest` (§4) partly mitigates it — an
auditor cannot be walked into a certificate that is _already_ fraudulent — but it
does nothing about a reserve withdrawn after attestation, which is precisely the
case `InsufficientReserve` exists for. The residue is real, it is an open
problem, and no fix is invented here. It needs a deliberate decision. Candidate
directions, none chosen: requiring the challenger not to be the certificate's own
operator (weak — addresses are free); routing a self-challenge's slash back to
the auditor rather than the treasury; or treating an operator-initiated
`InsufficientReserve` proof as hygiene too, which trades this griefing vector for
a way for an operator to escape a real reserve shortfall.

### Gaps deliberately left open

- **`ExpiredCertificate` judges one payment, not a history.** The router records
  a single post-expiry pair — the largest late payment and its timestamp — so a
  large payment inside the grace window masks a smaller, later one that would
  have cleared both tests. Conservative in the safe direction (fewer upholds),
  and closing it would put unbounded storage on the router's x402 hot path.
- **The renewal check is coarser than §7's wording.** §7 says "not renewed
  _before_ the payment"; the predicate asks "not renewed as of resolution", so a
  renewal filed _after_ the late payment also defeats the proof. That is a cure
  path in the sense of §2, and §2's evaluate-at-filing machinery does not exist
  yet. It errs toward not killing a certificate.
- **The grace window and the floor are unresearched proposals.** 24 hours and
  0.1% are §7's numbers, adopted as-is. Both are attack surface: the window is
  free post-expiry coverage a hostile operator can plan around, and the floor is
  a band of payments that provably breach the covenant and are unprovable anyway.
- **`set_router` has no deploy-script caller yet.** `scripts/deploy.ts` wires the
  contracts through `initialize` only; until it also calls `set_router`, a fresh
  deployment resolves both new predicates into `router_not_set`.
- **No premium step.** Step 4 is a comment, not code. No PremiumVault exists and
  none was built.
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
- **`publish`** gains vault-allowlist validation (§3).
- **Challenge lifecycle** grows from two states to four: `Open`, `Upheld`,
  `Rejected`, `Cured` (§2), with an aggregating claim window (§1).
- **A new admin/governance surface** with a timelock and a seal (§5).
- **`VerifyResult` / `CertView`** gain `tracked` and the float cap (§6, §8).

Four of these are storage-layout changes to the certificate itself, which is why
this is a redeploy and not a patch.

## Still genuinely open

- §3: whether removing a vault hash from the allowlist invalidates existing
  certificates. Leaning no; must be decided before implementation.
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
