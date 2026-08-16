// stellar.expert URL builders. Client-safe. Follows the deployment's network
// passphrase, so it points at the mainnet explorer the day one exists.
import { IS_PUBLIC_NETWORK } from "./ui-config";

const net = IS_PUBLIC_NETWORK ? "public" : "testnet";
const BASE = `https://stellar.expert/explorer/${net}`;

export const explorer = {
  account: (address: string) => `${BASE}/account/${address}`,
  tx: (hash: string) => `${BASE}/tx/${hash}`,
  contract: (contractId: string) => `${BASE}/contract/${contractId}`,
};
