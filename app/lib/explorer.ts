// stellar.expert URL builders. Client-safe. Defaults to the testnet explorer;
// swap the segment if NETWORK ever becomes "public".
import { NETWORK } from "./ui-config";

const net = NETWORK === "public" ? "public" : "testnet";
const BASE = `https://stellar.expert/explorer/${net}`;

export const explorer = {
  account: (address: string) => `${BASE}/account/${address}`,
  tx: (hash: string) => `${BASE}/tx/${hash}`,
  contract: (contractId: string) => `${BASE}/contract/${contractId}`,
};
