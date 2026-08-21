# @bound/sdk

Typed TypeScript client for [Bound Protocol](https://github.com/iamnotdou/bound) — five
Soroban smart contracts on Stellar that give an AI agent a certificate: a pre-funded
reserve, an independent auditor's stake, and a permissionless way to challenge the claim
if it's false. This package is the client over that system: it wraps the generated
contract bindings, exposes the committed deployment addresses, and provides read helpers
(verify a certificate, check balances/stakes) and write helpers (stake, deposit, publish,
attest, pay, challenge) for callers who hold a Stellar keypair.

## Install

```bash
npm i @bound/sdk
```

`@stellar/stellar-sdk` is a regular dependency, so it is installed for you. The generated
contract bindings are bundled into this package — there is nothing else to install.

## Status

This targets **Stellar testnet only**. The five contract addresses, network endpoints,
and demo actor public keys are committed inside the package (`@bound/sdk/deployments`) —
there is no mainnet deployment yet, and no configuration is needed to point at testnet.

## Usage

The default entry (`@bound/sdk`) is server-side: it pulls in `@stellar/stellar-sdk` and
all six generated contract clients to reach the chain. A minimal read — verifying a
certificate — needs no keypair at all:

```ts
import { bound, toCertView } from "@bound/sdk";

const result = await bound.verifyCertificate("G...AGENT_PUBLIC_KEY");
console.log(toCertView("G...AGENT_PUBLIC_KEY", result));
// { agent, valid, status, boundUsd, reserveUsd, auditorStakeUsd, auditor, ... }
```

`bound` is a ready-to-use `BoundClient` instance pointed at the committed testnet
deployment. `verifyCertificate` simulates a read against the chain (no signature
required) and returns the raw `VerifyResult`; `toCertView` turns that into a
JSON-safe, UI-friendly shape (bigint fields formatted as USD strings).

Other read-only calls on `BoundClient` follow the same pattern — no keypair needed:

|                 |                                                                                                        |
| --------------- | ------------------------------------------------------------------------------------------------------ |
| Certificates    | `certIdForAgent`, `reserveBalance`, `usdcBalance`                                                      |
| Auditors        | `auditorStake`, `auditorRegistered`, `auditorMinStake`                                                 |
| The spend meter | `routedCertId`, `routedBalance`, `spendForCert`, `floatForCert`, `floatCapForCert`, `certHalted`       |
| Coverage        | `quotePremium`, `quotePremiumForCert`, `premiumPaid`, `coverage`, `premiumAccrued`, `premiumClaimable` |
| Claim windows   | `windowClosesAt`, `claimWindowSettled`, `claimWindowSeconds`                                           |

`spendForCert` returns **gross routed flow, not loss**. A certificate that has
routed more than its bound has broken a covenant about its own conduct; it has
not thereby lost anyone that much money, and the contract sizes no payout from
it. Rendering that number as harm would be the single easiest way to misread
this protocol.

### Writing transactions

`BoundClient` also exposes methods that sign and submit transactions —
`stakeAsAuditor`, `depositReserve`, `depositFee`, `publishCertificate`,
`attestCertificate`, `mintUsdc`, `executePayment`, `enrollAgent`, `fundFloat`,
`withdrawFloat`, `haltCert`, `resumeCert`, `payPremium`, `claimPremium`,
`challengeCertificate`, `closeClaimWindow`. Each takes a `Keypair` (from
`@stellar/stellar-sdk`) as its first argument and signs locally before submitting.
**This is a server-side-only path**: it requires the caller's own secret key material,
so it belongs behind your own backend, not in a browser bundle or anywhere a secret key
would be exposed to a client.

Two of them take **two** keypairs, and the reason is the same both times: neither
party may conscript the other.

- `publishCertificate(operator, agent, …)` — the registry authenticates the agent as
  well as the operator, so nobody can be bonded without consenting to it.
- `enrollAgent(operator, agent, …)` — enrollment attaches spend to the operator's
  certificate and puts the agent's address under the operator's kill switch.

Soroban permits one contract call per transaction, so both signatures land in the same
envelope: the operator signs the transaction and the agent signs its own authorization
entry. A UI cannot split either into two submissions.

### Routed payments

```ts
await bound.enrollAgent(operator, agent, certId, usdc(200)); // 200 = the float cap
const receipt = await bound.executePayment(agent, recipient, usdc(50));
receipt.routed; // true — this payment moved the meter
```

**`routed` is the field to check.** `executePayment` sends an enrolled signer's
payment through the PaymentRouter, and only then is it metered. An unenrolled signer
falls back to the raw USDC asset contract: the payment still lands, it just leaves no
trace on the counter a challenger reads, so `BoundExceeded` could never be proven
against it. The method reports which rail it took rather than quietly picking one.

The router holds custody of its own float, so a routed payment spends the signer's
_router_ balance; a shortfall is deposited first unless you pass `{ autoFund: false }`.
Topping up on demand rather than parking a balance is the safer default — the float cap
bounds what a stolen agent key can reach, and float that is never idle cannot be stolen.

### `@bound/sdk/deployments`

A second, client-safe entry point:

```ts
import { getDeployment } from "@bound/sdk/deployments";

const deployment = getDeployment(); // defaults to "testnet"
deployment.contracts.registry; // C...
deployment.rpcUrl; // Soroban RPC endpoint
```

Import this instead of the default entry from any code that ships to a browser. It
returns the committed deployment record — network endpoints, the eight contract ids, the
RPC read-source account, and the demo actors' public keys — without pulling in
`Keypair` or any of the generated contract clients, so it doesn't drag the full chain
client into a client bundle.

## What's in this package

- `BoundClient` (exported as the `bound` singleton, and the `BoundClient` class) — the
  typed facade over the registry, reserve vault, auditor staking, fee escrow, challenge
  manager, payment router, premium vault, and USDC token contracts.
- `toCertView` / `CertView` — a JSON-safe projection of a certificate for HTTP or UI use.
- `agentFetch` — x402-style helper: fetch a URL, and if it responds `402 Payment
Required`, pay the demanded amount and retry with proof. It pays through
  `executePayment`, so an enrolled agent's x402 spend is metered like any other. The
  rail is deliberately not baked in: a facilitator-based x402 integration settles into
  the same spend-tracking hook rather than replacing it.
- `usdc` / `formatUsdc` — convert between dollar amounts and the 7-decimal USDC stroop
  amounts the contracts expect.
- `getDeployment`, `listNetworks`, deployment types — the committed deployment record.
- The raw generated contract clients (re-exported from `./bindings`), for callers who
  need a method `BoundClient` doesn't wrap.

## Learn more

Source, contracts, and full documentation: <https://github.com/iamnotdou/bound>
