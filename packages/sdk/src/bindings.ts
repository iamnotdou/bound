// Re-exports of the generated contract bindings, so a consumer of @bound/sdk can
// reach the raw typed clients without depending on `bindings/*` — which it could
// not do anyway, since those packages are never published and two of them are
// named after unrelated packages on npm (`registry`, `usdc`).
//
// These are named re-exports rather than `export *` on purpose. Every generated
// module ends with `export * from '@stellar/stellar-sdk'`, so six star exports
// would collide on hundreds of names and each generated module also exports its
// own `networks` and `DataKey`. Listing the contract-specific symbols keeps the
// SDK's surface the contracts' surface.
//
// Generated code — regenerate `bindings/`, then update the lists below.
export {
  Client as RegistryClient,
  networks as registryNetworks,
  type Certificate,
  type CertStatus,
  type DataKey as RegistryDataKey,
  type VerifyResult,
} from "../../../bindings/registry/src";

export {
  Client as ReserveVaultClient,
  networks as reserveVaultNetworks,
  type DataKey as ReserveVaultDataKey,
} from "../../../bindings/reserve_vault/src";

export {
  Client as AuditorStakingClient,
  networks as auditorStakingNetworks,
  type DataKey as AuditorStakingDataKey,
} from "../../../bindings/auditor_staking/src";

export {
  Client as FeeEscrowClient,
  networks as feeEscrowNetworks,
  type DataKey as FeeEscrowDataKey,
} from "../../../bindings/fee_escrow/src";

export {
  Client as ChallengeManagerClient,
  networks as challengeManagerNetworks,
  type Challenge,
  type DataKey as ChallengeManagerDataKey,
  type ProofType,
  type Verdict,
} from "../../../bindings/challenge_manager/src";

export { Client as UsdcClient, networks as usdcNetworks } from "../../../bindings/usdc/src";
