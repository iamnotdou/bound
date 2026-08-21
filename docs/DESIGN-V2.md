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
