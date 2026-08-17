// @bound/sdk — the typed client for Bound Protocol.
//
// Server-side entry point: it reaches the chain, so it pulls in
// @stellar/stellar-sdk and all six generated contract bindings. Browser code
// that only needs addresses should import `@bound/sdk/deployments` instead,
// which carries the committed deployment data and nothing else.
//
// No secrets live in this package. Keypairs are supplied by the caller.

// Committed deployment data (also available on its own at `@bound/sdk/deployments`).
export * from "./deployments";

// Network endpoints, contract addresses, and the USDC money helpers.
export * from "./config";

// The chain client. `VerifyResult` and `ProofType` are named explicitly so this
// re-export shadows the star export from "./bindings" below rather than
// colliding with it.
export { BoundClient, bound } from "./bound-client";
export type { VerifyResult, ProofType } from "./bound-client";

// JSON-safe certificate projection for HTTP and UI boundaries.
export * from "./cert-view";

// x402 — autonomous payment for an HTTP resource.
export * from "./payments";

// The raw generated contract clients, for callers that need a method BoundClient
// does not wrap.
export * from "./bindings";
