// Runs before any test module is imported.
//
// app/lib/config.ts resolves contract addresses at *import* time: it throws on a
// missing variable, and if REGISTRY_ADDRESS is unset it lazily loads
// .env.testnet from disk. Either behaviour would make the unit suite depend on a
// real deployment, so we seed placeholder values here.
//
// Setting REGISTRY_ADDRESS is what suppresses the dotenv read, which is the part
// that matters: it guarantees the suite never touches .env.testnet and never
// reads a real address or secret. These are the all-zero strkeys: real,
// checksum-valid Stellar IDs that address nothing, so they survive any format
// validation without ever naming a live contract or account.
//
// This shim disappears at plan step 3.3, when config.ts starts reading committed
// deployment data instead of the environment.
const PLACEHOLDER_CONTRACT = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4";
const PLACEHOLDER_ACCOUNT = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF";

const stub: Record<string, string> = {
  REGISTRY_ADDRESS: PLACEHOLDER_CONTRACT,
  RESERVE_VAULT_ADDRESS: PLACEHOLDER_CONTRACT,
  AUDITOR_STAKING_ADDRESS: PLACEHOLDER_CONTRACT,
  FEE_ESCROW_ADDRESS: PLACEHOLDER_CONTRACT,
  CHALLENGE_MANAGER_ADDRESS: PLACEHOLDER_CONTRACT,
  USDC_ADDRESS: PLACEHOLDER_CONTRACT,
  OPERATOR_ADDRESS: PLACEHOLDER_ACCOUNT,
};

for (const [key, value] of Object.entries(stub)) {
  process.env[key] = value;
}
