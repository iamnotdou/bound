import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    // `eslint .` covers the whole repo, so generated and emitted code has to be
    // excluded explicitly or it dominates the report.
    ignores: [
      "**/node_modules/**",
      // tsup output for packages/* — bundled, not authored here.
      "packages/*/dist/**",
      // cargo output.
      "target/**",
      // Generated from the contract wasm by the Stellar CLI. Regenerated with
      // `build:bindings`, never edited by hand, so linting them reports on code
      // nobody can fix here.
      "bindings/**",
      "**/*.tsbuildinfo",
    ],
  },
  // The repo is Rust contracts, a published TypeScript SDK, an MCP server and
  // the deploy/demo scripts — all Node. It previously extended
  // `eslint-config-next`, which only made sense while a Next.js app lived in
  // `apps/`; the frontend now lives in its own repo (bound-web) and carries its
  // own Next config, so this one lints plain TypeScript and nothing else.
  ...tseslint.configs.recommended,
  {
    // The scripts and MCP server use deliberate `any` at a few chain-boundary
    // seams and a lazy `require("dotenv")` for non-bundled contexts. Keep these
    // as warnings so they report without blocking a build.
    rules: {
      "@typescript-eslint/no-explicit-any": "warn",
      "@typescript-eslint/no-require-imports": "warn",
      // Honour the leading-underscore convention for deliberately unused
      // bindings, so a parameter kept for call-site clarity can stay without
      // being deleted to satisfy the linter.
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],
    },
  },
);
