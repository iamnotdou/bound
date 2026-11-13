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
   `bound-v1.boundproto-nh26.testnet` and method `pay` (0.25 NEAR gas
   allowance), then deletes its full-access key. From then on NEAR itself
   refuses anything else the key tries, before any contract runs:
   - a call with any deposit (`ft_transfer`, `withdraw`):
     `AccessKeyDepositWithFunctionCallActionError`
   - a call to another contract (`usdc....mint`):
     `AccessKeyReceiverMismatchActionError`
   - a NEAR transfer: `AccessKeyRequiresFullAccessActionError`
     These produce no transaction at all, which the demo log says explicitly.
     `view_access_key_list` shows exactly one key, and anyone can check it.
2. **Named accounts.** `verify("bound-agent-....testnet")` is readable by a
   person and by an agent; the certificate names the operator, auditor and
   agent by account id.
3. **NEP-141 custody and NEP-297 receipts.** Every `pay` emits
   `EVENT_JSON:{"standard":"bound","version":"1.0.0","event":"pay","data":[...]}`
   which the gate reads from RPC as a payment receipt.

### Limits of the agent's key

The key cannot be widened by the agent: adding keys needs a full-access key,
and the agent has none. What the key CAN still do: call `pay` on any
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

| Area                     | Soroban                                                             | NEAR port                                                                                                               | Why                                                                                                                                                                                                                                                      |
| ------------------------ | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Topology                 | 7 contracts, sync calls                                             | 1 contract                                                                                                              | async receipts, atomic settlement (section 1)                                                                                                                                                                                                            |
| Agent consent            | operator + agent sign `publish` together                            | `publish` (operator) then `accept` (agent)                                                                              | NEAR has no multi-signer tx. A Pending cert never blocks the agent, so publishing certs that name someone else's agent is harmless (`pending_cert_does_not_block_agent`). Fixes Soroban `defect_publish_overwrites_agent_mapping_without_agent_consent`. |
| Init                     | `initialize` callable by anyone first                               | `#[init]` in the same tx as deploy                                                                                      | closes `defect_initialize_requires_no_authorization` (deploy txs in `deployments/testnet.json`)                                                                                                                                                          |
| Rejected trustless claim | bond kept in the contract's bond pool                               | bond credited to `treasury` in the same call                                                                            | explicit and visible                                                                                                                                                                                                                                     |
| Hygiene bounty           | paid from the pool of forfeited bonds                               | paid from the certificate's reserve, `min(hygiene_bounty, reserve)`, split equally                                      | forfeited bonds go to the treasury here, so there is no pool; the operator whose agent broke the covenant pays it                                                                                                                                        |
| Premium                  | `PremiumVault`: rate x duration, linear accrual, forfeit to victims | flat premium escrowed at `publish`, paid at `attest`: 90% auditor, 10% treasury (`protocol_fee_bps`)                    | 24 h scope (plan item N0). Waterfall step 4 (forfeited premium) therefore does not exist                                                                                                                                                                 |
| `InsufficientReserve`    | can become true when the vault balance drops                        | structurally false after `attest` (reserve is locked inside the same contract and only leaves by settlement or release) | kept so the false-claim path is real; `cure` was not ported because there is nothing to cure                                                                                                                                                             |
| Router custody           | router holds USDC, enrolls a copy of the cert                       | float is a field of the cert                                                                                            | one contract                                                                                                                                                                                                                                             |
| Bound                    | `spent > bound` is a predicate, router does not block at bound      | same: `pay` is capped by the float, not the bound                                                                       | `spent` is gross flow, not loss (see AGENTS.md); `BoundExceeded` stays a hygiene predicate                                                                                                                                                               |
| Windows                  | per-cert claim window, unbounded claims                             | max 16 claims per window (`WindowFull`)                                                                                 | bounds `close_window` gas; the 17th claimant waits for the next window                                                                                                                                                                                   |
| Float on settlement      | n/a                                                                 | unspent float returns to the operator; reserve remainder by `release_reserve`                                           |                                                                                                                                                                                                                                                          |
| Fake signature proof     | `FakeSignature`                                                     | `Harm { claimed, evidence_uri }`, decided by the arbiter, `harm <= claimed`                                             | the claim is about harm, not signatures                                                                                                                                                                                                                  |

Waterfall steps 1-3 and 5 are ported 1:1 in `src/settle.rs` as a pure function:

```
payable      = min(total_harm, reserve + allocation)
reserve_draw = min(payable, reserve)
victim_pool  = min(reserve_draw, arbitrated_harm)  -> victims pro rata; rest -> treasury
fee_pool     = min(fee_bps x total_harm, reserve - reserve_draw) -> challengers pro rata
slash        = min(payable - reserve_draw, allocation) -> treasury only
allocation - slash -> auditor free stake; cert Invalid; admitted bonds returned
```

The auditor is slashed only for harm the operator's own reserve could not
cover, and the slash never goes to a claimant (anti self-dealing).

## 5. Test map

Unit tests: `near/contracts/bound/src/tests.rs` (88) and `src/settle.rs` (8).
Sandbox tests: `near/contracts/bound/tests/sandbox.rs` (2). Mock token: 4.

| NEAR test                                                                                           | Corresponds to Soroban test (`contracts/integration-tests/tests/cross_contract.rs`)                                         |
| --------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `settle::harm_above_reserve_slashes_the_difference`, `demo_vector_harm_above_reserve`               | `insufficient_reserve_fraud_draws_the_reserve_first_then_the_allocation`                                                    |
| `settle::harm_above_reserve_plus_allocation_is_capped`, `harm_above_reserve_plus_allocation_capped` | `slash_is_capped_by_the_certificates_allocation`                                                                            |
| `settle::single_victim_harm_below_reserve_no_slash`, `harm_below_reserve_no_slash_fee_from_reserve` | `slash_is_capped_by_proven_harm`                                                                                            |
| `settle::predicate_share_goes_to_treasury_not_victim`                                               | `insufficient_reserve_slash_goes_to_the_treasury_and_never_to_victim_or_challenger`                                         |
| `settle::two_victims_pro_rata`, `two_victims_pro_rata`                                              | (new) pro-rata split of one window                                                                                          |
| `false_insufficient_reserve_rejected_bond_to_treasury`                                              | `insufficient_reserve_no_fraud_forfeits_the_challengers_bond`, `over_funded_reserve_is_not_fraud`                           |
| `challenge_bond_too_small_rejected`                                                                 | `challenge_below_minimum_bond_is_rejected_without_moving_money`                                                             |
| `bound_exceeded_settles_in_hygiene_mode`                                                            | `hygiene_mode_kills_the_certificate_and_pays_a_flat_bounty_without_touching_the_reserve` (bounty source differs, section 4) |
| `verify_reads_expired_after_expiry`                                                                 | `certificate_becomes_invalid_after_expiry`                                                                                  |
| `release_reserve_before_deadline_rejected`, `release_allocation_before_deadline_rejected`           | `reserve_and_allocation_stay_locked_through_the_post_expiry_challenge_window`                                               |
| `release_after_deadline_returns_collateral`                                                         | `clean_expiry_returns_stake_and_reserve`                                                                                    |
| `attest_below_min_stake_rejected`, `attest_above_free_stake_rejected`                               | `under_staked_auditor_cannot_attest`                                                                                        |
| `attest_by_other_rejected`                                                                          | `attest_rejects_a_signature_from_the_wrong_address`                                                                         |
| `pending_cert_does_not_block_agent`, `second_live_cert_rejected`, `accept_by_other_rejected`        | `defect_publish_overwrites_agent_mapping_without_agent_consent`                                                             |
| sandbox `withdraw_failure_recredits_and_guards_hold` (second `new` refused)                         | `defect_initialize_requires_no_authorization`                                                                               |
| `verify_returns_verified_view`, sandbox `full_lifecycle_settles_victim`                             | `happy_path_publishes_funds_attests_and_verifies`                                                                           |

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
- **Contract upgrade key.** `bound-v1.boundproto-nh26.testnet` keeps the
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
- **Victims are paid up to the reserve.** The auditor's slash goes to the
  treasury, so a victim whose harm exceeds the reserve is not made whole.
- **Not protected:** harm the arbiter cannot assess off chain; float drained to
  allowlisted recipients (section 3); storage deposits are not refundable.

## 8. Failure modes

| Failure                                                                      | Effect                                     | Handling                                                                         |
| ---------------------------------------------------------------------------- | ------------------------------------------ | -------------------------------------------------------------------------------- |
| Token `ft_transfer` fails on withdraw (receiver not registered on the token) | tokens stay in the contract                | callback re-credits; withdraw again after `storage_deposit` on the token         |
| Arbiter never decides a Harm claim                                           | window closes with the claim Unadjudicated | bond returned, cert unfrozen                                                     |
| 17th claim in one window                                                     | `WindowFull` panic                         | nothing moves; file after the window closes                                      |
| Window passes with no admitted claim                                         | cert unfrozen and payable again            | `close_window` returns null                                                      |
| Agent FC key gas allowance used up                                           | agent cannot pay                           | operator issues a new agent account and cert                                     |
| Faucet rate limit (seen during the build: 429 after ~11 accounts)            | `near:demo` cannot create actors           | automatic fallback to the `testnet` registrar funded by a local key (`--funder`) |
| `rpc.testnet.near.org` deprecated (answers -429)                             | reads fail                                 | both RPC entries are FastNEAR hostnames, failover between them                   |
