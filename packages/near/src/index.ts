// @bound/near: read and verify Bound certificates on NEAR testnet.
// Library entry. The MCP server is at "@bound/near/mcp"; the CLI is
// `bound-near`. Nothing here needs a key; `call` / `tryCall` sign only with
// an Account the caller builds.

export { TESTNET, type TestnetDeployment } from "./testnet.js";
export {
  checkConfinement,
  getAgentCertificate,
  getCertificate,
  verifyAgent,
  type AgentVerdict,
  type Confinement,
  type ReadOptions,
} from "./read.js";
export {
  TESTNET_ARCHIVAL_RPCS,
  TESTNET_RPCS,
  accountCodeOf,
  accountFromKey,
  call,
  chainNow,
  codeHashOf,
  decodeOutcome,
  errorText,
  panicText,
  testnetProvider,
  tryCall,
  view,
  withRetry,
  type CallOpts,
  type TxOutcome,
} from "./client.js";
export * from "./format.js";
export { quietRpcRetryLogs } from "./quiet.js";
export { VERSION } from "./version.js";
