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
`certIdForAgent`, `usdcBalance`, `reserveBalance`, `auditorStake`,
`auditorRegistered`, `auditorMinStake`.

### Writing transactions

`BoundClient` also exposes methods that sign and submit transactions —
`stakeAsAuditor`, `depositReserve`, `depositFee`, `publishCertificate`,
`attestCertificate`, `mintUsdc`, `executePayment`, `challengeCertificate`,
`resolveChallenge`. Each of these takes a `Keypair` (from `@stellar/stellar-sdk`) as
its first argument and signs locally before submitting. **This is a server-side-only
path**: it requires the caller's own secret key material, so it belongs behind your own
backend, not in a browser bundle or anywhere a secret key would be exposed to a client.

### `@bound/sdk/deployments`

A second, client-safe entry point:

```ts
import { getDeployment } from "@bound/sdk/deployments";

const deployment = getDeployment(); // defaults to "testnet"
deployment.contracts.registry; // C...
deployment.rpcUrl; // Soroban RPC endpoint
```

Import this instead of the default entry from any code that ships to a browser. It
returns the committed deployment record — network endpoints, the six contract ids, the
RPC read-source account, and the demo actors' public keys — without pulling in
`Keypair` or any of the six generated contract clients, so it doesn't drag the full chain
client into a client bundle.

## What's in this package

- `BoundClient` (exported as the `bound` singleton, and the `BoundClient` class) — the
  typed facade over the registry, reserve vault, auditor staking, fee escrow, challenge
  manager, and USDC token contracts.
- `toCertView` / `CertView` — a JSON-safe projection of a certificate for HTTP or UI use.
- `agentFetch` — x402-style helper: fetch a URL, and if it responds `402 Payment
Required`, pay the demanded amount via USDC and retry with proof.
- `usdc` / `formatUsdc` — convert between dollar amounts and the 7-decimal USDC stroop
  amounts the contracts expect.
- `getDeployment`, `listNetworks`, deployment types — the committed deployment record.
- The raw generated contract clients (re-exported from `./bindings`), for callers who
  need a method `BoundClient` doesn't wrap.

## Learn more

Source, contracts, and full documentation: <https://github.com/iamnotdou/bound>
