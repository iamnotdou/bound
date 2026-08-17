import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig({
  resolve: {
    // The unit suite runs against SDK *source*, not packages/sdk/dist — so
    // `pnpm test` needs no build and a test failure points at a line you can
    // edit. Longest specifier first: Vite matches these as prefixes.
    alias: {
      "@bound/sdk/deployments": here("./packages/sdk/src/deployments.ts"),
      "@bound/sdk": here("./packages/sdk/src/index.ts"),
    },
  },
  test: {
    environment: "node",
    // Seeds placeholder contract addresses so importing the SDK's config never
    // reads .env.testnet. See test/setup-env.ts.
    setupFiles: ["./test/setup-env.ts"],
    include: ["**/*.test.ts"],
    exclude: [
      "**/node_modules/**",
      ".next/**",
      "apps/*/.next/**",
      "packages/*/dist/**",
      "target/**",
      "bindings/**",
    ],
  },
});
