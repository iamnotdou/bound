/**
 * Conventional commits, enforced locally by the commit-msg hook.
 *
 * CI lints PR *titles* against this same config (see .github/workflows/ci.yml).
 * That matters because we squash-merge: squashing discards individual commit
 * messages and writes the PR title as the single commit on main. Sharing one
 * config is what stops the two from ever disagreeing.
 */
module.exports = {
  extends: ["@commitlint/config-conventional"],
  rules: {
    "type-enum": [
      2,
      "always",
      [
        "feat",
        "fix",
        "chore",
        "docs",
        "refactor",
        "test",
        "ci",
        "style",
        "perf",
        "build",
        "revert",
      ],
    ],
  },
};
