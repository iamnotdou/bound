import { dirname } from "path";
import { fileURLToPath } from "url";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const compat = new FlatCompat({
  baseDirectory: __dirname,
});

const eslintConfig = [
  ...compat.extends("next/core-web-vitals", "next/typescript"),
  {
    // The verified backend SDK (app/lib, scripts, mcp) uses deliberate `any` at
    // a few chain-boundary seams and a lazy `require("dotenv")` for non-Next
    // contexts. Keep these as warnings so they don't block `next build`.
    rules: {
      "@typescript-eslint/no-explicit-any": "warn",
      "@typescript-eslint/no-require-imports": "warn",
    },
  },
];

export default eslintConfig;
