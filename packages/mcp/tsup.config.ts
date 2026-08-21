import { defineConfig } from "tsup";

// Unlike @bound/sdk, nothing here lives outside the package: the tools reach
// the chain through `@bound/sdk`, which is a real npm package and stays
// external. So there is nothing to inline, and tsup's default — externalise
// everything declared in `dependencies` — is exactly right.
export default defineConfig({
  entry: {
    // The embedder entry: tool table + server factory.
    index: "src/index.ts",
    // The tool definitions alone. apps/dashboard imports this to drive them
    // through the AI SDK, and must not pull an MCP server into a Next build.
    tools: "src/tools.ts",
    // The executable named by `bin`. Its leading `#!/usr/bin/env node` is
    // carried through from src/bin.ts — esbuild preserves an entry hashbang.
    "bound-mcp": "src/bin.ts",
  },
  format: ["esm", "cjs"],
  dts: true,
  sourcemap: true,
  clean: true,
  treeshake: true,
  platform: "node",
  target: "node20",
  // accounts.ts reaches dotenv through `createRequire(import.meta.url)`, which
  // has no meaning in the CJS output. The shim gives it one.
  shims: true,
  external: [/^node:/],
});
