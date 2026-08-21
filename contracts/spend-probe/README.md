# spend-probe

**Not a deployable contract. Do not add to `deployments/`, do not reference from the SDK.**

An executable specification for the PaymentRouter's spend counter. It exists to
pin down one claim before the router is written:

> Cumulative routed spend measures **gross flow**, not **loss**.

`exceeded()` implements the naive `BoundExceeded` predicate exactly as the
milestone states it — `spent > bound`. The tests then break it:

| test                                                             | shows                                                                                                   |
| ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `naive_spend_predicate_is_manufacturable_with_no_net_flow`       | a $1 shuttle between two addresses the operator controls drives `spent` past any bound at zero net cost |
| `payee_tally_does_not_prove_harm`                                | crediting the payee instead is worse — being paid is not being harmed, and the operator can pay itself  |
| `harm_bounded_settlement_pays_nothing_for_a_manufactured_breach` | settlement driven by **proven harm** pays zero on the manufactured breach                               |
| `genuine_overspend_to_a_third_party_produces_real_harm`          | the same predicate on a real counterparty does produce harm                                             |
| `payout_is_capped_by_collateral`                                 | payout never exceeds reserve + per-certificate allocation                                               |

The conclusion the router must encode: `BoundExceeded` proves **the covenant was
broken**, not **that anyone was harmed**. Compensation is driven by proven harm,
capped by collateral. The counter is evidence, not a payout trigger.

Bounds in the tests are $10 to keep test snapshots small; the shuttle is
O(bound/unit) host operations. Comments carry the real-world figures.
