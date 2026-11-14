// pnpm --filter @bound/near sync-deployment -- writes data/testnet.json, the
// public deployment summary the npm package bundles, from the full record in
// near/deployments/testnet.json. Ids and hashes only, no keys. A vitest case
// fails when the two drift apart.

import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { loadDeployment } from "../tooling/deployment.js";
import { summarize } from "../tooling/summary.js";

const out = join(import.meta.dirname, "..", "data", "testnet.json");
writeFileSync(out, JSON.stringify(summarize(loadDeployment()), null, 2) + "\n");
console.log(`wrote ${out}`);
