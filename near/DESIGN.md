# Bound on NEAR: design notes

Built for NEAR HACKATHON (2026-11). This file records how the Soroban protocol
in `contracts/` was ported, where it deliberately differs, the trust model, and
the limits. Code: `near/contracts/bound` (protocol), `near/contracts/mock-usdc`
(test token), `packages/near` (client, demo, gate).

## 1. One contract, and why

NEAR cross-contract calls are asynchronous: a callee's failure does not roll
back the caller, and a chain of promises needs explicit compensation logic. The
Soroban protocol is seven contracts that call each other synchronously
(registry -> auditor staking on attest; challenge manager -> vault, staking,
registry on settlement). Ported as seven NEAR contracts, a settlement would be
a multi-receipt saga that can stop half way.

So on NEAR the registry, reserve vault, auditor staking, payment router and
challenge manager are ONE contract (`bound`). Settlement (`close_window`) is a
single state change in a single receipt. The only outgoing promise in the whole
contract is `withdraw` -> token `ft_transfer`, which debits first and re-credits
in a `#[private]` callback if the transfer fails.

This is a constraint we designed around, not a NEAR advantage. What is NEAR
specific is in section 3.

## 2. Money model

- Tokens enter only by NEP-141 `ft_transfer_call` -> `ft_on_transfer`, which
  accepts only the configured token and credits an internal balance. Optional
  `msg` actions do the next move in the same call:
  `{"stake":{}}`, `{"reserve":{"cert_id":N}}`, `{"float":{"cert_id":N}}`,
  `{"credit":{"account_id":"x"}}`. A malformed msg or failed action panics, so
  the token refunds the whole transfer. An unregistered beneficiary gets the
  full amount refunded (the contract never creates state at its own expense).
- Inside the protocol only internal balances move (reserve, float, stake,
  allocation, bonds, payouts). No per-payout promises, so nothing can fail half
  way.
- Tokens leave only by `withdraw` (pull, exactly 1 yoctoNEAR, >= 30 Tgas).
- Storage: minimal NEP-145. `storage_deposit` registers a balance for a fixed
  0.01 NEAR. `publish` needs 0.05 NEAR attached, `challenge` 0.01 NEAR. Each of
  these methods measures `env::storage_usage()` before and after and panics
  (`StorageNotCovered`) if the bytes written cost more than the deposit, so the
  fixed amounts are enforced, not assumed (`storage_deposit_covers_usage`).
  Deposits are not refunded in this version.
- `pay` requires the recipient to be registered, so an agent cannot make the
  contract pay storage by spraying payments at fresh accounts.

## 3. What NEAR gives Bound that a wrapper cannot

1. **Protocol-level key confinement.** The demo agent accepts its certificate
   with its full-access key, adds a function-call key scoped to receiver
   `bound-v2.boundproto-nh26.testnet` and method `pay` (0.25 NEAR gas
   allowance), then deletes its full-access key. From then on NEAR itself
   refuses anything else the key tries, before any contract runs:
   - a call with any deposit (`ft_transfer`, `withdraw`):
     `AccessKeyDepositWithFunctionCallActionError`
   - a call to another contract (`usdc....mint`):
     `AccessKeyReceiverMismatchActionError`
   - a NEAR transfer: `AccessKeyRequiresFullAccessActionError`
     These produce no transaction at all, which the demo log says explicitly.
     `view_access_key_list` shows exactly one key, and `view_account` shows
     `code_hash` `11111111111111111111111111111111` (no contract code on the
     agent account). Anyone can check both.
2. **Named accounts.** `verify("bound-agent-....testnet")` is readable by a
   person and by an agent; the certificate names the operator, auditor and
   agent by account id.
3. **NEP-141 custody and NEP-297 receipts.** Every `pay` emits
   `EVENT_JSON:{"standard":"bound","version":"1.0.0","event":"pay","data":[...]}`
   which the gate reads from RPC as a payment receipt.

### Limits of the agent's key

Keys are only half of confinement. Contract code deployed on the agent
account runs with `predecessor = agent` and needs no key at all: it could call
`pay`, call `ft_transfer` on the token, or add a full-access key to its own
account. So an agent that deployed code before deleting its full-access key is
NOT confined, whatever its key list says. Confined therefore means both:

1. every access key is a function-call key to the Bound contract, methods
   exactly `["pay"]`, and
2. `view_account(agent).code_hash == "11111111111111111111111111111111"`.

`isConfined` in `packages/near/src/format.ts`, the demo (act 3), `near:verify`,
the MCP tool `near_check_agent_keys` and the web page all check both. The Bound
contract cannot read another account's keys or code, so `verify(agent)` alone
does NOT prove confinement: a counterparty reads `verify` and both RPC views.
With no full-access key and no code, the key cannot be widened (adding a key
needs a full-access key or code on the account). What the key CAN still do: call `pay` on any
certificate (the contract rejects every cert where the caller is not the
agent), spend its 0.25 NEAR gas allowance on failing calls (gas griefing of its
own account only), and pay up to the current float to an allowlisted
recipient. That last item is the residual risk: a hijacked agent can drain the
float to an allowlisted counterparty. It is capped by `float_cap` and stopped
by the operator's `halt`. If no allowlist is set, the float can go to any
registered account.

The agent account can never be recovered after its full-access key is deleted
(no key can add a key). That is the point; it is also why demo agents are
throwaway accounts.

## 4. Deviations from the Soroban source

| Area                     | Soroban                                                             | NEAR port                                                                                                                                                       | Why                                                                                                                                                                                                                                                      |
| ------------------------ | ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Topology                 | 7 contracts, sync calls                                             | 1 contract                                                                                                                                                      | async receipts, atomic settlement (section 1)                                                                                                                                                                                                            |
| Agent consent            | operator + agent sign `publish` together                            | `publish` (operator) then `accept` (agent)                                                                                                                      | NEAR has no multi-signer tx. A Pending cert never blocks the agent, so publishing certs that name someone else's agent is harmless (`pending_cert_does_not_block_agent`). Fixes Soroban `defect_publish_overwrites_agent_mapping_without_agent_consent`. |
| Init                     | `initialize` callable by anyone first                               | `#[init]` in the same tx as deploy                                                                                                                              | closes `defect_initialize_requires_no_authorization` (deploy txs in `deployments/testnet.json`)                                                                                                                                                          |
| Rejected trustless claim | bond kept in the contract's bond pool                               | bond credited to `treasury` in the same call                                                                                                                    | explicit and visible                                                                                                                                                                                                                                     |
| Hygiene bounty           | paid from the pool of forfeited bonds                               | paid from the certificate's reserve, `min(hygiene_bounty, reserve)`, split equally                                                                              | forfeited bonds go to the treasury here, so there is no pool; the operator whose agent broke the covenant pays it                                                                                                                                        |
| Premium                  | `PremiumVault`: rate x duration, linear accrual, forfeit to victims | flat premium escrowed at `publish`, paid at `attest`: 90% auditor, 10% treasury (`protocol_fee_bps`)                                                            | 24 h scope (plan item N0). Waterfall step 4 (forfeited premium) therefore does not exist                                                                                                                                                                 |
| `InsufficientReserve`    | can become true when the vault balance drops                        | structurally false after `attest` (reserve is locked inside the same contract and only leaves by settlement or release)                                         | Unreachable on NEAR: a claim of it is always rejected. It is kept only for ABI parity with Soroban; the demo's false claim uses `BoundExceeded`, which can be true or false. `cure` was not ported because there is nothing to cure                      |
| Slash recipient          | the whole slash goes to the treasury                                | the slash pays unpaid ARBITRATED harm first, pro rata; only the rest goes to the treasury                                                                       | a victim whose harm exceeds the reserve is made whole up to `coverage = reserve + allocation`. Only arbiter-assessed victims receive it, so a predicate claimant still cannot self-deal                                                                  |
| Unadjudicated claim      | bond returned                                                       | half the bond forfeited to the treasury                                                                                                                         | freezing a certificate must cost something; otherwise anyone can freeze it every window for free                                                                                                                                                         |
| Who may challenge        | anyone                                                              | anyone except the cert's operator, agent and auditor (`ConflictedChallenger`)                                                                                   | stops the operator filling the 16 claim slots with junk to lock real victims out                                                                                                                                                                         |
| Late claims              | fixed window                                                        | a claim with less than a third of a window left extends the window to now + window/3                                                                            | a late victim (and the arbiter) always get time                                                                                                                                                                                                          |
| Arbiter grace            | n/a                                                                 | `close_window` refuses (`ArbiterGrace`) while a Harm claim is undecided, until closes_at + window/3                                                             | nobody can front-run the arbiter by closing at the exact second                                                                                                                                                                                          |
| Payments after deadline  | router stops at expiry + window                                     | `pay` refused from the settlement deadline on (`SettlementDeadlinePassed`), and only on the cert `verify(agent)` reports (`NotCurrentCert`)                     | spend that can no longer be challenged is not allowed; what a counterparty reads is what meters the agent                                                                                                                                                |
| Ids                      | u32/u64 per contract                                                | ABI ids are u64; any id above u32::MAX is `NoSuchCert` / `NoSuchChallenge`; frozen = a window exists for the cert's own id                                      | bound-v1 truncated ids (`as u32`), so `2^32` aliased cert 0 with its own window (fixed in bound-v2, section 9)                                                                                                                                           |
| Router custody           | router holds USDC, enrolls a copy of the cert                       | float is a field of the cert                                                                                                                                    | one contract                                                                                                                                                                                                                                             |
| Bound                    | `spent > bound` is a predicate, router does not block at bound      | same: `pay` is capped by the float, not the bound                                                                                                               | `spent` is gross flow, not loss (see AGENTS.md); `BoundExceeded` stays a hygiene predicate                                                                                                                                                               |
| Windows                  | per-cert claim window, unbounded claims                             | max 16 claims per window (`WindowFull`); an arbiter-rejected claim frees its slot at once; one claim per trustless predicate kind per window (`DuplicateClaim`) | bounds `close_window` gas without letting junk claims squat the slots (section 10)                                                                                                                                                                       |
| Float on settlement      | n/a                                                                 | unspent float returns to the operator; what is left of the reserve and the allocation stays locked until the settlement deadline                                | a settled cert still answers later Harm claims (section 10)                                                                                                                                                                                              |
| Fake signature proof     | `FakeSignature`                                                     | `Harm { claimed, evidence_uri }`, decided by the arbiter, `harm <= claimed`                                                                                     | the claim is about harm, not signatures                                                                                                                                                                                                                  |

Waterfall steps 1-3 and 5 are ported 1:1 in `src/settle.rs` as a pure function:

```
payable      = min(total_harm, reserve + allocation)
reserve_draw = min(payable, reserve)
victim_pool  = min(reserve_draw, arbitrated_harm)  -> victims pro rata; rest -> treasury
fee_pool     = min(fee_bps x total_harm, reserve - reserve_draw) -> challengers pro rata
slash        = min(payable - reserve_draw, allocation)
               -> unpaid arbitrated harm first (pro rata), rest -> treasury
allocation - slash stays allocated until the deadline; cert Invalid (halted);
               admitted bonds returned
```

The auditor is slashed only for harm the operator's own reserve could not
cover. Unlike Soroban, the slash first tops up arbitrated victims who were not
paid in full; it never goes to a predicate claimant or a challenger fee.

What a counterparty can rely on is `coverage = reserve_locked + allocation`
(returned by `verify`): the most all arbitrated victims of one certificate can
recover together. `bound` is not a guarantee: it is a spend meter, and
`spent` can pass it by refilling the float (that is what `BoundExceeded`
catches). The demo's gate therefore extends credit up to
`min(bound, coverage) - outstanding invoices`.

## 5. Test map

Unit tests: `near/contracts/bound/src/tests.rs` (109) and `src/settle.rs` (9).
Sandbox tests: `near/contracts/bound/tests/sandbox.rs` (2). Mock token: 4.

| NEAR test                                                                                                                                                         | Corresponds to Soroban test (`contracts/integration-tests/tests/cross_contract.rs`)                                         |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `settle::harm_above_reserve_slashes_the_difference`, `demo_vector_harm_above_reserve`                                                                             | `insufficient_reserve_fraud_draws_the_reserve_first_then_the_allocation`                                                    |
| `settle::harm_above_reserve_plus_allocation_is_capped`, `harm_above_reserve_plus_allocation_capped`                                                               | `slash_is_capped_by_the_certificates_allocation`                                                                            |
| `settle::single_victim_harm_below_reserve_no_slash`, `harm_below_reserve_no_slash_fee_from_reserve`                                                               | `slash_is_capped_by_proven_harm`                                                                                            |
| `settle::predicate_share_goes_to_treasury_not_victim`                                                                                                             | `insufficient_reserve_slash_goes_to_the_treasury_and_never_to_victim_or_challenger`                                         |
| `settle::two_victims_pro_rata`, `two_victims_pro_rata`                                                                                                            | (new) pro-rata split of one window                                                                                          |
| `false_insufficient_reserve_rejected_bond_to_treasury`                                                                                                            | `insufficient_reserve_no_fraud_forfeits_the_challengers_bond`, `over_funded_reserve_is_not_fraud`                           |
| `challenge_bond_too_small_rejected`                                                                                                                               | `challenge_below_minimum_bond_is_rejected_without_moving_money`                                                             |
| `bound_exceeded_settles_in_hygiene_mode`                                                                                                                          | `hygiene_mode_kills_the_certificate_and_pays_a_flat_bounty_without_touching_the_reserve` (bounty source differs, section 4) |
| `verify_reads_expired_after_expiry`                                                                                                                               | `certificate_becomes_invalid_after_expiry`                                                                                  |
| `release_reserve_before_deadline_rejected`, `release_allocation_before_deadline_rejected`                                                                         | `reserve_and_allocation_stay_locked_through_the_post_expiry_challenge_window`                                               |
| `release_after_deadline_returns_collateral`                                                                                                                       | `clean_expiry_returns_stake_and_reserve`                                                                                    |
| `attest_below_min_stake_rejected`, `attest_above_free_stake_rejected`                                                                                             | `under_staked_auditor_cannot_attest`                                                                                        |
| `attest_by_other_rejected`                                                                                                                                        | `attest_rejects_a_signature_from_the_wrong_address`                                                                         |
| `pending_cert_does_not_block_agent`, `second_live_cert_rejected`, `accept_by_other_rejected`                                                                      | `defect_publish_overwrites_agent_mapping_without_agent_consent`                                                             |
| sandbox `withdraw_failure_recredits_and_guards_hold` (second `new` refused)                                                                                       | `defect_initialize_requires_no_authorization`                                                                               |
| `verify_returns_verified_view`, sandbox `full_lifecycle_settles_victim`                                                                                           | `happy_path_publishes_funds_attests_and_verifies`                                                                           |
| `alias_cert_id_rejected_everywhere`, `collateral_stays_locked_while_window_open`, `close_window_refuses_settled_cert`, `window_cap_holds`                         | (new, judge review B1) id aliasing regression tests                                                                         |
| `conflicted_challenger_rejected`, `grief_freeze_costs_bond`, `late_claim_extends_window`, `arbiter_grace_blocks_front_run`                                        | (new, judge review M3) claim-window griefing                                                                                |
| `premium_refunded_when_reserve_never_funded`, `pay_after_deadline_rejected`, `verify_reports_coverage`, `settle::slash_beyond_unpaid_harm_goes_to_treasury`       | (new, judge review m1, m2, M1)                                                                                              |
| `c1_hygiene_settlement_keeps_liability_until_deadline`, `c1_tiny_fraud_settlement_keeps_liability_until_deadline`, `settled_cert_refuses_new_predicate_challenge` | (new, review 2026-11-14 C1, section 10)                                                                                     |
| `c2_rejected_claims_free_slots_and_late_victim_is_heard`, `c2_duplicate_trustless_claim_refused`, `deadline_boundary_is_consistent`                               | (new, review 2026-11-14 C2 and deadline boundary)                                                                           |
| `huge_harm_capped_at_coverage`, `conflicted_victim_rejected`, `excess_deposit_refunded`, `halted_or_empty_float_is_not_a_deal`, `superseded_cert_cannot_pay`      | (new, review 2026-11-14 minor findings)                                                                                     |

## 6. Security checklist

| Item                                                                                                                | Where                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `ft_on_transfer` only from the configured token                                                                     | `ft_on_transfer_from_other_token_rejected`, sandbox                                                      |
| msg parse error panics so the token refunds                                                                         | `ft_on_transfer_bad_msg_panics_so_token_refunds`, sandbox                                                |
| unregistered beneficiary refunded, no state created                                                                 | `ft_on_transfer_refunds_unregistered_beneficiary`, sandbox                                               |
| withdraw debits before the promise, callback re-credits on failure                                                  | `withdraw_debits_before_promise`, sandbox `withdraw_failure_recredits_and_guards_hold`                   |
| withdraw requires 1 yocto and >= 30 Tgas                                                                            | `withdraw_without_yocto_rejected`, `withdraw_with_low_gas_rejected`                                      |
| every mutating method needs 1 yocto or its storage deposit, except `pay`, `ft_on_transfer` and the private callback | `assert_one_yocto()` at the top of each; `accept_without_yocto_rejected`, `*_deposit_too_small_rejected` |
| roles checked with `predecessor_account_id`, never `signer_account_id`                                              | `grep signer_account_id near/contracts/bound/src/lib.rs` finds nothing                                   |
| checked u128 arithmetic, `overflow-checks = true` in release                                                        | `near/Cargo.toml`                                                                                        |
| no unbounded iteration: `list_certs` limit <= 50, <= 16 claims per window                                           | `list_certs_paginates`, `seventeenth_claim_rejected`                                                     |
| storage deposits cover actual usage                                                                                 | `storage_deposit_covers_usage` and the runtime `StorageNotCovered` check                                 |
| no admin setters; config immutable                                                                                  | no setter exists; `get_config`                                                                           |
| `pay` cannot take a deposit (not `#[payable]`), so a FC key can call it                                             | near-sdk rejects a deposit on non-payable methods                                                        |

## 7. Trust model and what is not protected

- **Arbiter.** One named account (`arbiter.boundproto-nh26.testnet`)
  controlled by the author decides `Harm` claims. It could uphold a fake claim
  up to `reserve + allocation` of any certificate. Production mitigation: a
  per-certificate arbiter chosen at `publish`, or a DAO (Sputnik) arbiter.
- **One team holds every trusted role.** The arbiter, the treasury and the
  contract's full-access key are all the author's accounts. The arbiter could
  uphold a fake Harm claim naming a colluding victim and drain any
  certificate up to its coverage; forfeited bonds and leftover slash go to the
  author's treasury. This is a testnet demo trust model, not a production one.
  Roadmap: a per-certificate arbiter named at `publish`, or a Sputnik DAO
  arbiter, a DAO-held treasury, and deleting the contract key.
- **Contract upgrade key.** `bound-v2.boundproto-nh26.testnet` keeps the
  deployer's full-access key during judging so a bug can be fixed by deploying
  to a fresh account. That key could replace the code. Check that the code on
  chain is the code in this repo: `pnpm near:status` compares the on-chain
  `code_hash` (base58 sha256 of the wasm) with the hash recorded at deploy, and
  `near/scripts/verify.sh` rebuilds the wasm and compares its sha256 with the
  recorded one. Production: delete the key or hand it to a DAO.
- **Demo timers.** Claim window 90 s, post-expiry 300 s, grace 60 s. Production
  defaults would be 72 h / 7 d / 24 h. `get_config` returns the live values.
- **Mock USDC** has an open `mint`: it is play money, not Circle USDC.
- **Spend is not loss.** `spent` is gross routed flow.
- **Victims are paid up to coverage.** `reserve + allocation`, shared pro
  rata by every arbitrated victim of the certificate.
- **Credit across services can be oversubscribed.** Each gate limits its own
  credit by `min(bound, coverage)`, but two services that each extend that
  much share one coverage pool.
- **`verify(agent)` returns the agent's last ACCEPTED certificate,** and only
  that certificate can pay. An older cert reads `Superseded` and can no
  longer spend, but it stays challengeable until its own settlement deadline;
  read it with `get_cert` or `list_certs`.
- **Not protected:** harm the arbiter cannot assess off chain; float drained to
  allowlisted recipients (section 3); storage deposits are not refundable.

## 8. Failure modes

| Failure                                                                      | Effect                                     | Handling                                                                                                  |
| ---------------------------------------------------------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| Token `ft_transfer` fails on withdraw (receiver not registered on the token) | tokens stay in the contract                | callback re-credits; withdraw again after `storage_deposit` on the token                                  |
| Arbiter never decides a Harm claim                                           | window closes with the claim Unadjudicated | after the arbiter grace: half the bond returned, half to the treasury, cert unfrozen                      |
| 17th claim in one window                                                     | `WindowFull` panic                         | nothing moves; file again once the arbiter rejects a claim (its slot is freed) or after the window closes |
| Window passes with no admitted claim                                         | cert unfrozen and payable again            | `close_window` returns null                                                                               |
| Agent FC key gas allowance used up                                           | agent cannot pay                           | operator issues a new agent account and cert                                                              |
| Faucet rate limit (seen during the build: 429 after ~11 accounts)            | `near:demo` cannot create actors           | automatic fallback to the `testnet` registrar funded by a local key (`--funder`)                          |
| `rpc.testnet.near.org` deprecated (answers -429)                             | reads fail                                 | both RPC entries are FastNEAR hostnames, failover between them                                            |

## 9. bound-v1 (superseded) and the certificates on it

The first deployment, `bound-v1.boundproto-nh26.testnet`, truncated u64 ids to
u32 (`cert_id as u32`). Found in the pre-submission review: cert id `2^32`
resolved to cert 0 but opened its own claim window, and closing that alias
window unfroze cert 0 while the real window was still open, so collateral
could leave before an upheld victim was paid. It also bypassed the 16-claim
cap. Fixed in `bound-v2.boundproto-nh26.testnet` (section 4, Ids), together
with the other review fixes in this file. bound-v1 is left as it was so its
evidence stays checkable (`previous` in `deployments/testnet.json`,
`evidence/near-demo-mwfgi0ij.*`, `evidence/near-demo-mwfgqr0u.*`,
`evidence/mcp-agent-mwfgqr0u.*`); do not use it.

Certificates 0-2 on bound-v1 came from two early development runs
(`mwffu37a`, `mwfgaomc`) that failed part way, before the evidence format
existed; cert 2 stayed in `Accepted` (never attested, so nothing is locked).
They hold no evidence and are not part of the demo. bound-v2 starts from
cert 0 with the run recorded in `deployments/testnet.json`.

## 10. Review fixes (2026-11-14)

A pre-submission security review of bound-v2 found two major and six minor
contract issues. Each one has a regression test in `src/tests.rs` that
failed on the reviewed code and passes now (section 5). The fixes are in the
repository; **they are not deployed yet**: `bound-v2.boundproto-nh26.testnet`
still runs the reviewed code until it is redeployed. The borsh layout of the
stored state did not change (only the JSON view gained a field).

**C1, major: the operator could end its liability early.** With no
allowlist, the agent could loop the float through an operator sybil until
`spent > bound`; the sybil filed `BoundExceeded`, one window later the hygiene
settlement made the cert `Invalid` and released the reserve and the
auditor's allocation at once, and every later victim got `NotChallengeable`
long before `expires_at + post_expiry`. A fraud settlement over a tiny real
harm did the same. Now:

- a settlement (hygiene or fraud) halts the cert for spending but leaves it
  open to `Harm` claims until `deadline()`; later windows pay from what is
  left of the reserve and the allocation;
- `release_reserve` on an `Invalid` cert waits for the deadline, and
  `close_window` no longer frees the allocation: the slashed part leaves the
  auditor's stake, the rest stays allocated until `release_allocation`;
- trustless predicates are refused on a settled cert, so the hygiene bounty
  cannot be drawn twice. `Settlement.auditor_returned` keeps its name but now
  means "allocation left unslashed, released at the deadline".

**C2, major: claim-slot squatting at the deadline.** Sixteen minimum-bond
`Harm` claims filed just before the deadline filled the only window; claims
the arbiter rejected kept their slots, the window closed after the deadline,
and the real victim was locked out. Now an arbiter-rejected claim settles its
bond to the treasury and leaves the window at once, and after the deadline a
claim is still accepted into a window that opened before it and is still open
(`now < closes_at`). No new window opens from the deadline on. True trustless
predicates could squat too, so a window takes one claim per predicate kind.

**Minor:**

- _Harm overflow._ A proven harm near `u128::MAX` overflowed the waterfall and
  made `close_window` panic forever. `resolve_by_arbiter` caps it at
  `reserve_locked + allocation`.
- _Deadline boundary._ At `now == deadline` the allocation could be released
  and a claim then accepted against it. Release needs `now >= deadline`;
  challenge and `pay` now need `now < deadline` (or, for a claim, the open
  pre-deadline window above).
- _Conflicted victim._ The named victim may not be the operator, the agent or
  the auditor (`ConflictedVictim`), the same check the challenger had.
- _Excess deposit._ NEAR attached to `publish` or `challenge` above the
  required deposit is refunded to the caller.
- _Halted view._ The view read `Verified` while the cert was halted or its
  float was 0. Status is now `Halted` while halted, and the new `deal` field is
  true only when status is `Verified` and the float is above 0.
- _Superseded cert._ `verify(agent)` reports the newest accepted cert while an
  older one could still pay until its deadline. Of the two options, the
  simpler one was taken: `pay` refuses any cert other than the one `verify`
  reports (`NotCurrentCert`), and such a cert reads `Superseded`.

**Remaining limitations, stated honestly:**

- Freeing a squatted slot depends on the arbiter rejecting the junk claim in
  time. An arbiter that does not act leaves the slots full until the window
  closes, and after the deadline that still locks a late victim out.
- After the deadline each accepted claim can still extend the open window by
  a third of a window. Someone can keep the collateral locked a little longer
  that way, but every rejected claim costs its full bond.
- The hygiene bounty still goes to whoever files the true predicate, which
  can be the operator's own sybil. It is paid from the operator's own reserve,
  so it is cost-neutral to the operator, but it no longer ends any liability.
- A cert settled more than once keeps only the latest `Settlement` in its
  view; the `settle` events carry every one.
- New view values (`Halted`, `Superseded`, `deal`). A client that acts only
  on status `Verified` already treats them as "do not deal"; a client that
  looks for specific bad statuses has to learn the two new ones.
- Demo timers are unchanged (claim window 90 s, post-expiry 300 s), so the
  demo still runs in minutes.
