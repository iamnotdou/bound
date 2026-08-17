import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    // Seeds placeholder contract addresses so importing app/lib/config.ts never
    // reads .env.testnet. See test/setup-env.ts.
    setupFiles: ["./test/setup-env.ts"],
    include: ["**/*.test.ts"],
    exclude: ["**/node_modules/**", ".next/**", "apps/*/.next/**", "target/**", "bindings/**"],
  },
});
