# Threat model: custody and the agent key

`docs/DESIGN-V2.md` § 6 makes this document a precondition of shipping custody,
not a follow-up to it. The PaymentRouter holds funds, so the question "what can
someone who steals an agent's key actually reach?" has to have a written answer
before that contract is deployed, not after someone finds out.

Status: written against the router as built on `feat/payment-router`.
Nothing is deployed.

---

## What custody changes

Before the router, an agent key is a Stellar account key. Steal it and you drain
that account. The blast radius is whatever the agent was holding.

With the router, the agent transacts in **wrapped USDC** held in the router's
custody and metered per certificate. Steal the key and you reach the agent's
wrapped balance too — the operating float. That is a real increase in what a
single compromised key is worth, and it is the reason this document exists.

The mitigation is not that theft becomes impossible. It is that the reachable
amount is **bounded, visible, and stoppable**:

- **Bounded** by the per-certificate float cap, set at enrolment.
- **Visible** because the cap is on the certificate, so a counterparty can see
  the maximum exposure before trusting the agent.
- **Stoppable** by the operator's kill switch, which halts routing without a
  challenge and which the agent key cannot clear.

---

## What a stolen agent key can reach

| Reachable                                        | Why                                                                                                               |
| ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| The agent's wrapped balance, up to the float cap | The thief holds the key that authorizes `transfer`. This is the loss.                                             |
| Any destination                                  | `transfer` takes an arbitrary `to`. There is no allowlist of payees, and there should not be one — see below.     |
| The spend counter                                | Every stolen-key transfer meters against the certificate like any other. A thief can push `spent` past the bound. |

## What it cannot reach

| Out of reach                  | Why                                                                                                                                                           |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| The reserve                   | `ReserveVault::deposit` and `release_to_operator` authenticate the **certificate's operator**, read from the Registry. The agent key is not the operator key. |
| The auditor's stake           | Only a proven challenge moves it, and only through the challenge manager.                                                                                     |
| Any other certificate's float | Balances, caps and halts are keyed by certificate id.                                                                                                         |
| Float above the cap           | Deposits beyond the cap are rejected, so the operator cannot accidentally raise the ceiling by topping up.                                                    |
| The kill switch               | Halt and resume authenticate the operator. A thief holding the agent key cannot resume routing after the operator halts it. This is tested explicitly.        |
| The certificate itself        | Nothing the agent key can do invalidates or re-points a certificate.                                                                                          |

**Why no payee allowlist.** It is the obvious mitigation and it is rejected on
purpose. An agent that can only pay a fixed set of addresses is not an
autonomous agent, and the operator maintaining that list becomes a trusted party
in every transaction — which is what the protocol exists to remove. The float cap
bounds the same loss without constraining who the agent may deal with.

---

## Who bears the loss

This is the part that must not be got wrong.

**A compromised agent key is not a slashable auditor fault.**

The auditor attests that capital is committed and that the operator's process is
sound. They do not attest to the operator's key hygiene, they cannot observe it,
and they cannot price it. An auditor who can be slashed for an operator losing a
key is underwriting a risk they have no instrument to measure — and the rational
response to that is to not audit at all, or to demand a premium that makes the
whole product pointless. Getting this boundary wrong makes auditing uninsurable.

So:

- **The operator bears float loss** from their own compromised key. The float cap
  is the operator's own risk limit, chosen by them.
- **The bond covers harm to counterparties**, whoever caused it. If a thief uses
  the stolen key to take payment and not deliver, the counterparty was harmed by
  the agent, and that is exactly the case the bond exists for. The counterparty
  does not have to care whose fault the key loss was, and must not be asked to.
- **The auditor is slashed only on a proven proof**, on the same terms as any
  other challenge. Key compromise is not itself a proof type.

The distinction that carries this: **the operator's own loss and a counterparty's
loss are different things**, and only the second is covered. A thief draining the
float harms the operator, and the protocol pays nothing. A thief harming a
counterparty triggers the ordinary settlement waterfall.

---

## The consequence for the spend counter

A thief can push `spent` past the bound, which makes `BoundExceeded` true without
anyone having been harmed. That is the same result `spend-probe` proves for a
self-dealing operator, arrived at by a different route, and it has the same
answer: **the counter is evidence, not a payout trigger**, and compensation is
driven by proven harm and capped by collateral.

If `BoundExceeded` ever paid out on the counter alone, stealing an agent key
would become a way to force a payout from an honest operator's collateral. It
does not, and this is one of the reasons why.

`BoundExceeded` is now provable trustlessly — `resolve` reads the router's
counter directly, with no arbiter — and it settles in **hygiene mode**: the
certificate is invalidated, the challenger is paid a flat bounty out of forfeited
bonds, the reserve is untouched, and **the auditor is not slashed**. So a thief
who pushes `spent` past the bound can kill the certificate, which is the correct
outcome and the same outcome a compromised agent's own overspending should have.
They cannot reach the auditor's allocation, and neither can the operator.

---

## An operator can still grief their own auditor

This is the most important **unclosed** item on this page, and it is stated here
rather than buried in a design doc because it is a live economic exposure for
anyone considering auditing on this protocol.

The settlement waterfall makes manufacturing a proof **unprofitable**. It does
not make `InsufficientReserve` **costless to the auditor**.

An operator who deliberately under-funds their own certificate and then
challenges it destroys their auditor's allocation. The slashed stake goes to the
treasury, so the operator gains nothing — they are out their gas, their reserve
and their certificate. The auditor is out their whole allocation regardless. The
integration harness demonstrates exactly this sequence today: the colluders
extract nothing, and the auditor's allocation lands in the treasury.

The minimum-reserve check at `attest` mitigates the attestation-time version of
this — an auditor cannot be walked into a certificate that is already fraudulent
— but a reserve withdrawn _after_ attestation is precisely the case
`InsufficientReserve` exists for, and nothing stops the operator doing that to
themselves.

**No fix is proposed here.** It needs a deliberate decision, and every obvious
candidate trades one exposure for another: refusing a challenge from the
certificate's own operator is defeated by a second address; routing a
self-challenge's slash back to the auditor requires deciding what "self" means;
settling `InsufficientReserve` as hygiene too would give an operator a way to
walk away from a real reserve shortfall. Until one is chosen, an auditor's
downside on any certificate is bounded by their allocation but is **not** under
their own control.

---

## Compromise response

The intended sequence, and why it is ordered this way:

1. **Operator halts routing** for the certificate. This is deliberately the first
   step: it requires no challenge, no proof, no counterparty, and no auditor. Any
   response that depended on the challenge system would be too slow to matter and
   would put a compromise on the same clock as a dispute.
2. **Operator claws the remaining float back.** `clawback(cert_id, agent)` is
   operator-only, works **only while the certificate is halted**, and sweeps the
   named agent's whole router balance to the operator's address. It performs no
   resume, so the thief is never re-enabled — recovering the money and re-opening
   the certificate stay two separate acts. The destination is not a parameter:
   it is the certificate's registered operator and nothing else. It is a purely
   internal balance move, so `total_supply()` still equals the router's USDC
   balance; the operator withdraws from their own balance afterwards on their
   own authority. Whatever the thief has already taken is gone, so halting first
   is still what buys the time.
3. **The certificate remains valid.** A compromise is not a covenant breach, and
   invalidating on compromise would give an attacker a way to destroy a
   certificate by stealing a key.
4. **Counterparties harmed during the window challenge normally.** They are not
   asked to prove anything about the key.

---

## Known gaps

Stated rather than hidden:

- **Detection is out of scope.** Nothing here notices a compromise; it only
  bounds and stops one. The operator must be watching, and step 1 above is only
  as fast as they are.
- **The float cap is a per-certificate constant**, not a rate limit. A thief who
  drains a capped float, waits for the operator to refill, and drains it again
  reaches more than the cap in total. A velocity limit is the natural answer and
  is not designed yet.
- **A stolen key can pay a counterparty legitimately.** Nothing distinguishes a
  thief's honest payment from the agent's, which is correct — but it means the
  spend counter cannot be used to date a compromise.
- **An operator can sweep their own agent's float at will.** This is the residue
  of `clawback`, and it is stated rather than hidden. Outside a compromise,
  nothing stops an operator halting their own certificate and clawing the float
  back the same second. **It is acceptable**, for one reason: it grants no
  authority over anyone else's money. The agent's routed balance is capital the
  operator deposited under a cap the operator set, and the operator could already
  reach all of it by halting and then withdrawing after a resume. Clawback
  removes the requirement to re-arm the thief on the way out; it does not widen
  who can be reached. What it does change is the _cost_ of the sweep to the
  operator — it is now one call with no window. **An agent is not a custodian and
  must never be treated as one.** Anyone extending the router so that a _third
  party_ can hold a balance under an enrolled agent's certificate must revisit
  `clawback` first, because that person's money would be inside the sweep.
- **Clawback is not timelocked, on purpose.** A timelock would be a delay handed
  to the attacker: the halt/clawback pair only works if response is faster than
  the thief, and a 24-hour delay gives an attacker who already holds the agent
  key 24 hours to find any exit the halt did not close. The usual reason to
  timelock a privileged sweep — giving depositors time to exit ahead of an
  abusive admin — does not apply, because there are no third-party depositors on
  that balance. If that ever stops being true, the previous bullet is where the
  decision has to be reopened.
- **Clawback sweeps one named agent per call.** A certificate may have several
  enrolled agents and the router keeps no certificate → agents index, so a
  compromise of two keys is two calls. An operator who has lost track of which
  addresses they enrolled will not be rescued by this function.
- **An idle certificate can still archive.** All eight contracts now extend
  storage TTLs on their write paths (defect L2), to 120 days, so an archived
  entry can no longer abort a transaction under normal use. No read-only path
  bumps anything, deliberately — making `verify` a fee-bearing write would be the
  wrong trade for the protocol's most-used call — so a certificate that nobody
  transacts against at all for 120 days does still archive. Any transaction
  against it resets the clock. TTL rent is paid by whoever submits the triggering
  transaction, never by the protocol.
- **`fee-escrow` is still deployed and still broken.** It is a singleton whose
  `Released` flag never resets, so it pays out exactly once ever, and the
  ChallengeManager stores its address without ever calling it (defect L3).
  Nothing on the settlement path reaches it, so it cannot misdirect protocol
  money — but it is a live contract that will accept a `deposit` and then be
  unable to release it a second time. **Do not deposit into it.** It should be
  deleted before the v2 redeploy; `docs/V2-CUTOVER.md` has the sequence and why
  that is blocked on an SDK change.
- **Inbound transfers are not cap-checked.** The float cap is enforced on
  `deposit` only. A tracked agent being _paid_ can exceed its cap, because
  refusing inbound payment would make an honest agent unable to be paid, and
  inbound value is not something a stolen key can conjure. The cap therefore
  bounds what the operator commits, not the maximum the agent can ever hold.
