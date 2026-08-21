# Quickstart

Two halves, split along the same line the protocol splits along.

**Reading** needs nothing: no account, no keys, no funding. Certificates,
reserves, auditor stakes, routed spend and coverage are public state, and
verifying them without asking anyone's permission is the whole point. Everything
under "Read the chain" runs from a clean clone.

**Writing** needs a funded testnet account, because publishing a certificate,
staking as an auditor or routing a payment are authenticated transactions.

---

## Requirements

- Node 22.14.0 (`.nvmrc` pins it; `nvm use` picks it up)
- pnpm
- For the contract half only: Rust 1.97.1 and the **`wasm32v1-none`** target

```bash
git clone https://github.com/iamnotdou/bound.git
cd bound
nvm use
pnpm install
pnpm build:sdk
```

`pnpm build:sdk` is not optional — the SDK's `dist/` is gitignored, so a fresh
clone has no built package until you run it.

`wasm32v1-none` is not interchangeable with `wasm32-unknown-unknown`. The latter
emits reference-types, which the Soroban host refuses at upload: contracts build
and every test passes, and then `stellar contract deploy` fails with
`Error(WasmVm, InvalidAction)`. `rust-toolchain.toml` installs both because
`cargo test` and clippy still want the host target.

---

## Read the chain

No configuration. The network defaults to testnet and the contract addresses come
from the committed deployment record, not from your environment.

### List every certificate

```bash
node --input-type=module -e "
const { listCertificates } = await import('./packages/sdk/dist/index.mjs');
for (const c of await listCertificates({ limit: 100 })) {
  console.log(c.certId, c.status, 'valid=' + c.valid, c.boundUsd, c.reserveUsd, c.auditorStakeUsd);
}"
```

Certificates are enumerated **by certificate id**, never by agent address.
`valid` is the only field a counterparty should gate on: `status` alone is not
enough, because an expired certificate keeps its `Verified` status forever.

### Check a reserve against its claim

```bash
node --input-type=module -e "
const { bound, getCertificate, formatUsdc } = await import('./packages/sdk/dist/index.mjs');
const certId = 1;
const cert = await getCertificate(certId);
console.log('claimed on the certificate:', cert.reserveUsd);
console.log('actually held in the vault:', formatUsdc(await bound.reserveBalance(BigInt(certId))));"
```

`reserveBalance` takes a certificate id, and that matters more than it looks.
The first revision kept **one pooled balance** for every certificate the vault
held, and the trustless proof compared that pooled number against a single
certificate's claim — so six certificates claiming $10,000 each all read as fully
backed against an $18,000 pool that covered 30% of them. Reserves are now keyed
per certificate, and `deposit` authenticates against _that certificate's_
operator, so nobody can fund or drain anyone else's.

### Read the spend meter

This is the number the `BoundExceeded` proof is made of:

```bash
node --input-type=module -e "
const { bound, formatUsdc } = await import('./packages/sdk/dist/index.mjs');
const certId = 1n;
console.log('routed spend:', formatUsdc(await bound.spendForCert(certId)));
console.log('float held  :', formatUsdc(await bound.floatForCert(certId)),
            'of', formatUsdc(await bound.floatCapForCert(certId)));"
```

Routed spend is **gross flow, not loss**. A certificate that has routed more than
its bound has broken a covenant about its own conduct; it has not thereby lost
anyone that much money, and the contract sizes no payout from it.

### Read the coverage economy

```bash
node --input-type=module -e "
const { bound, formatUsdc } = await import('./packages/sdk/dist/index.mjs');
const certId = 1n;
console.log('premium quote :', await bound.quotePremiumForCert(certId), 'stroops');
console.log('paid          :', await bound.premiumPaid(certId));
console.log('accrued yield :', await bound.premiumAccrued(certId), 'stroops');
console.log('claimable now :', await bound.premiumClaimable(certId), 'stroops');"
```

Premiums are priced `bound × rate × duration / 1 year` and accrue to the auditor
in a straight line across the certificate's term, less a protocol fee share that
leaves in the same transaction the premium is paid.

---

## Run the whole lifecycle yourself

```bash
pnpm run setup     # generate + friendbot-fund five demo accounts, mint test USDC
pnpm run deploy    # build and deploy all eight contracts, write deployments/testnet.json
pnpm run demo      # the lifecycle, live
```

`pnpm run demo` issues a certificate, funds its reserve, has an auditor attest
with their own slashable capital, buys coverage, enrolls a fresh agent in the
PaymentRouter, routes three payments through it until the metered spend passes
the bound, files a **false** claim and watches it get rejected and settled in the
same transaction, then files the **true** one and opens a claim window.

It generates a new agent keypair on every run, and has to: an enrollment is
permanent, so an operator cannot walk an agent off a certificate whose spend
counter is climbing and onto a clean one. A counter you can escape is not
evidence.

```bash
pnpm run demo:settle   # three days later
```

The claim window is 72 hours and that is not a placeholder. A challenge does not
settle when it is filed; it opens a window every other claimant against the same
certificate may join, and the whole set is priced together when the window
lapses. The first revision settled the first claim to arrive and foreclosed every
honest one behind it. `demo:settle` closes the window, settles every admitted
claim at once, and shows the auditor claiming their yield — it prints how long is
left rather than failing if you run it early.

The false claim, though, settles **inside `pnpm run demo`**, in the filing
transaction, with the bond forfeit. That asymmetry is the design: a predicate the
contract can evaluate itself needs no window when it comes out false, because
there is nothing to aggregate.

---

## Run the tests

Everything here is offline. No network, no testnet, no keys.

```bash
pnpm test            # TypeScript: SDK, config, discovery, transaction builders
pnpm test:contracts  # Rust: per-contract units plus the cross-contract harness
pnpm verify          # every gate at once, exactly as CI does
```

The cross-contract harness (`contracts/integration-tests`) registers every
contract plus a test token in a single `Env` and drives the real flows end to
end: each proof's fraud branch and its no-fraud branch, the settlement waterfall
with balance assertions and a conservation check, expiry, and authorization
rejection.

---

## Write to the chain

```ts
import { BoundClient, usdc } from "@bound/sdk";
import { Keypair } from "@stellar/stellar-sdk";

const bound = new BoundClient();

// Publishing authenticates the operator AND the agent, in one envelope: the
// operator signs the transaction, the agent signs its own authorization entry.
// Nobody can be bonded without consenting to it.
const certId = await bound.publishCertificate(operator, agent, {
  bound: usdc(500),
  reserveAmount: usdc(500),
  expiresAt: BigInt(Math.floor(Date.now() / 1000) + 30 * 24 * 3600),
});

await bound.depositReserve(operator, certId, usdc(500));
await bound.attestCertificate(auditor, certId, usdc(500)); // auditor bonds their own capital
await bound.payPremium(operator, certId); // coverage starts accruing

// Enrollment is what puts an agent on the metered rail. Both signatures again,
// for symmetric reasons: it attaches spend to the operator's certificate and
// puts the agent's address under the operator's kill switch.
await bound.enrollAgent(operator, agent, certId, usdc(200));

const receipt = await bound.executePayment(agent, recipient, usdc(50));
receipt.routed; // true — this payment moved the meter
```

**`routed` is the field to check.** An unenrolled signer's payment falls back to
the raw USDC asset contract. It still buys whatever it was buying; it just leaves
no trace on the counter a challenger reads, so `BoundExceeded` could never be
proven against it. `executePayment` tells you which one you got rather than
quietly picking.

### From a browser wallet

The transaction builders return an **unsigned, already-simulated envelope**, so
no secret key ever reaches the SDK:

```ts
import { buildActionXdr, submitSignedXdr } from "@bound/sdk";

const xdr = await buildActionXdr("publish", walletAddress, {
  agent: walletAddress,
  boundUsd: 50_000,
  reserveUsd: 10_000,
  expiryDays: 30,
});
// → wallet signs `xdr` in the browser →
const { hash, result } = await submitSignedXdr(signedXdr);
```

A browser wallet holds one key, so the only certificate it can publish unaided is
one naming itself. Bonding a _different_ agent needs that agent's signature in
the same envelope, and the builder refuses rather than producing a transaction
that would fail at submission.

### What each action needs

| Action                  | Who can do it                                                                        |
| ----------------------- | ------------------------------------------------------------------------------------ |
| Publish a certificate   | The operator and the agent, together. Records a **claimed** reserve; moves no money. |
| Fund a reserve          | That certificate's operator, and only them.                                          |
| Stake as an auditor     | Anyone, above the minimum stake.                                                     |
| Attest a certificate    | A registered auditor — and only against a reserve that is **actually funded**.       |
| Buy coverage            | That certificate's operator, once, and only once it is `Verified`.                   |
| Claim yield             | The certificate's auditor of record.                                                 |
| Enroll an agent         | The operator and the agent, together. Permanent.                                     |
| Halt / resume an agent  | That certificate's operator.                                                         |
| Challenge a certificate | Anyone, with a bond of their own money above the minimum.                            |
| Close a claim window    | Anyone, once it has lapsed. Pays the caller nothing.                                 |

That last row looks like a missing incentive and is not one: no claimant is paid
until somebody calls it, so every claimant has a reason to, and none of them can
be favoured by calling it first. A fee here would just be a race with a prize
attached.
