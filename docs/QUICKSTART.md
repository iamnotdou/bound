# Quickstart

Two halves, split along the same line the protocol splits along.

**Reading** needs nothing: no account, no keys, no funding. Certificates, reserves
and auditor stakes are public state, and verifying them is the whole point of the
protocol. Everything in "Read the chain" below runs from a clean clone.

**Writing** needs a funded testnet account and a wallet, because publishing a
certificate or staking as an auditor are authenticated transactions. That half is
covered at the end.

---

## Requirements

- Node 22.14.0 (`.nvmrc` pins it; `nvm use` picks it up)
- pnpm
- For the contract half only: Rust with the `wasm32-unknown-unknown` target

```bash
git clone https://github.com/iamnotdou/bound.git
cd bound
nvm use
pnpm install
pnpm build:sdk
```

`pnpm build:sdk` is not optional — the SDK's `dist/` is gitignored, so a fresh
clone has no built package until you run it.

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

Certificates are enumerated **by certificate id**, never by agent address. That is
deliberate: `publish` authenticates only the operator and then overwrites the
agent-to-certificate mapping unconditionally, so anyone can repoint any agent.
Listing by id is the only listing that cannot be tampered with.

Expect `Verified` certificates with `valid=false`. That is correct, not a bug:
`valid` also requires that the certificate has not expired, and the demo
certificates on testnet have.

### Verify one certificate

```bash
node --input-type=module -e "
const { getCertificate } = await import('./packages/sdk/dist/index.mjs');
console.log(await getCertificate(1));"
```

`valid` is the only field a counterparty should gate on. `status` alone is not
sufficient — an expired certificate keeps its `Verified` status forever.

### Check the reserve yourself — and see the defect

The trust model tells you to verify the reserve rather than trust a failed
challenge. Here is why, and you can reproduce it in one command:

```bash
node --input-type=module -e "
const { listCertificates, bound, formatUsdc } = await import('./packages/sdk/dist/index.mjs');
const certs = await listCertificates({ limit: 100 });
const vault = await bound.reserveBalance();
const claimed = certs.reduce((a, c) =>
  a + BigInt(Math.round(parseFloat(c.reserveUsd.replace(/[\$,]/g, '')) * 1e7)), 0n);
console.log('certificates        :', certs.length);
console.log('total claimed       :', formatUsdc(claimed));
console.log('actual vault balance:', formatUsdc(vault));
console.log('shortfall           :', formatUsdc(claimed - vault));"
```

At the time of writing this prints six certificates claiming **$60,000** of
reserve against a vault holding **$18,000** — a $42,000 shortfall.

Note that `reserveBalance()` takes **no certificate argument**. It cannot: the
deployed vault keeps a single balance for everything it holds, and the on-chain
reserve check compares that one pooled number against a single certificate's
claim. Every one of those six certificates therefore reads as fully backed, because
$18,000 exceeds any individual $10,000 claim — while collectively they are 70%
unbacked.

This is defect L5, it defeats the protocol's only trustless proof, and it is the
reason the next revision reworks reserve accounting to be per-certificate. Do not
build against the reserve check until then. See `docs/DESIGN-V2.md` § 9.

---

## Run the tests

Everything here is offline. No network, no testnet, no keys.

```bash
pnpm test            # TypeScript: SDK, config, discovery, transaction builders
pnpm test:contracts  # Rust: per-contract units plus the cross-contract harness
```

The cross-contract harness (`contracts/integration-tests`) registers all five
contracts plus a test token in a single `Env` and drives the real flows end to
end — the slash path asserting every balance movement and a conservation check,
the no-fraud branch asserting nothing moves, expiry, and authorization rejection.
It is also where L5 was found, and four known defects are pinned there as tests
asserting current behaviour so the suite records what the next revision must change.

To run every gate at once, exactly as CI does:

```bash
pnpm verify
```

---

## Write to the chain

This half needs a funded testnet account.

Reading is permissionless; writing is not, and two of these are more limited than
you might expect on the deployed contracts:

| Action                  | Who can do it today                                                                    |
| ----------------------- | -------------------------------------------------------------------------------------- |
| Publish a certificate   | Anyone with a funded account. Records a **claimed** reserve; moves no money.           |
| Stake as an auditor     | Anyone, above the minimum stake.                                                       |
| Attest a certificate    | A sufficiently staked auditor. Does **not** check that the reserve is actually funded. |
| Challenge a certificate | Anyone, with a bond.                                                                   |
| **Fund a reserve**      | **Only the single operator address stored in the vault at initialization.**            |

That last row is the same singleton defect as L5, seen from the other side. An
arbitrary operator cannot fund a reserve on the deployed contracts at all, which
means a certificate you publish yourself will be `Pending`, unfunded and
unattested. Publishing is still worth doing to see the flow — just do not expect
it to lock money, because it does not.

The transaction builders return an **unsigned, already-simulated envelope** for a
wallet to sign, so no secret key ever reaches the SDK:

```ts
import { buildActionXdr, submitSignedXdr } from "@bound/sdk";

const xdr = await buildActionXdr("publish", walletAddress, {
  agent: agentAddress,
  boundUsd: 50_000,
  reserveUsd: 10_000,
  expiryDays: 30,
});
// → wallet signs `xdr` in the browser →
const { hash, result } = await submitSignedXdr(signedXdr);
```

The connected wallet is both the transaction source and the `require_auth`
address, so one envelope signature is enough — Soroban needs no separate
auth-entry signing for this shape.
