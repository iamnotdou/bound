// Server-side transaction builders for the connected-wallet path.
//
// The implementation now lives in `@bound/sdk` (packages/sdk/src/tx.ts) so that
// any consumer of the published package can build the same unsigned envelopes.
// This module stays as the app's import path — a thin re-export, no behaviour
// of its own.
export {
  buildActionXdr,
  buildTrustlineXdr,
  submitSignedXdr,
  type WalletAction,
  type BuildParams,
} from "@bound/sdk";
