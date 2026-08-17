import { dirname } from "path";
import { fileURLToPath } from "url";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const compat = new FlatCompat({
  baseDirectory: __dirname,
});

const eslintConfig = [
  {
    // `next lint` only ever looked at the app source directories. Running
    // `eslint .` widens the scope to the whole repo, so build output and
    // generated code have to be excluded explicitly or they dominate the
    // report — .next alone accounts for ~2900 errors in emitted artifacts.
    ignores: [
      ".next/**",
      "apps/*/.next/**",
      "out/**",
      "apps/*/out/**",
      // tsup output for packages/sdk — bundled, minified-ish, not authored here.
      "packages/*/dist/**",
      "**/node_modules/**",
      "target/**",
      // Generated from the contract wasm by the Stellar CLI. Regenerated with
      // `build:bindings`, never edited by hand, so linting them reports on
      // code nobody can fix here.
      "bindings/**",
      // Written by Next on every build.
      "**/next-env.d.ts",
      "**/*.tsbuildinfo",
    ],
  },
  ...compat.extends("next/core-web-vitals", "next/typescript"),
  {
    // The verified backend SDK (apps/dashboard/app/lib, scripts, mcp) uses deliberate `any` at
    // a few chain-boundary seams and a lazy `require("dotenv")` for non-Next
    // contexts. Keep these as warnings so they don't block `next build`.
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
];

export default eslintConfig;
