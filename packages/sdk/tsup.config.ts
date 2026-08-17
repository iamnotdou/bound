import { defineConfig } from "tsup";

// The six generated binding packages. They are reached by relative path
// (`../../../bindings/<name>/src`) because two of them are named `registry` and
// `usdc`, which are real and unrelated packages on npm — see AGENTS.md.
//
// Nothing ever builds `bindings/*/dist`, so a plain `tsc` would emit those same
// relative specifiers into dist/ and the published tarball would import files
// that are not in it. They must be INLINED. esbuild already bundles relative
// imports unconditionally, so these patterns are belt-and-braces: they keep the
// rule true if the imports are ever switched to bare package names.
const BINDINGS = [
  "registry",
  "reserve_vault",
  "auditor_staking",
  "fee_escrow",
  "challenge_manager",
  "usdc",
];

export default defineConfig({
  entry: {
    // The full client. Server-side: it reaches the chain.
    index: "src/index.ts",
    // Committed deployment data on its own, for browser bundles that only need
    // addresses and must not pull in the chain client.
    deployments: "src/deployments.ts",
  },
  format: ["esm", "cjs"],
  dts: true,
  sourcemap: true,
  clean: true,
  treeshake: true,
  platform: "node",
  target: "node20",
  // Real, resolvable npm packages: they stay external and are declared as
  // dependencies. `buffer` is also a Node builtin; the generated bindings import
  // it by bare name, so it is declared for the benefit of browser bundlers.
  // Regexes, not bare strings: tsup matches a string `external` entry exactly,
  // which would miss the `@stellar/stellar-sdk/contract` and `/rpc` subpaths the
  // bindings import and silently inline the whole SDK.
  external: [/^@stellar\/stellar-sdk(\/|$)/, /^buffer$/, /^node:/],
  noExternal: BINDINGS.map((name) => new RegExp(`(^|/)bindings/${name}(/|$)`)),
});
