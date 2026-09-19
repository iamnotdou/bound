// The documents a stranger checks, checked.
//
// Every failure guarded here had already happened in this repo at least once,
// and each is the same shape: prose that reads as finished, states something
// specific, and falls over on the first click by someone deciding whether the
// project is real.
//
//   - the README's address table listed the v1 deployment, months after v2
//     replaced it, and omitted two contracts entirely
//   - ARCHITECTURE.md presented `deployments/testnet-anchor.json` in a table of
//     live deployments; the file does not exist
//   - a cited skill file pointed into `apps/dashboard/`, which moved to another
//     repository
//   - two `<!-- TODO before submission -->` markers sat in finished-looking
//     sections, rendering as nothing at all on GitHub
//
// Prose drifts silently. A test does not.
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

import { getDeployment, listNetworks } from "../packages/sdk/src/deployments";

const root = fileURLToPath(new URL("..", import.meta.url));

/** Every document a reader is pointed at, as `[path, contents]`. */
const DOCS: [string, string][] = [
  "README.md",
  "AGENTS.md",
  ...readdirSync(join(root, "docs"))
    .filter((f) => f.endsWith(".md"))
    .map((f) => `docs/${f}`),
].map((path) => [path, readFileSync(join(root, path), "utf8")]);

const deployment = getDeployment();
const deployedIds = Object.values(deployment.contracts).filter(
  (id): id is string => typeof id === "string",
);

describe("contract ids in the docs", () => {
  it("states every deployed contract somewhere a reader can find it", () => {
    const readme = readFileSync(join(root, "README.md"), "utf8");
    for (const [name, id] of Object.entries(deployment.contracts)) {
      if (!id) continue;
      expect(readme, `${name} (${id}) is missing from README.md`).toContain(id);
    }
  });

  it.each(DOCS)("%s names no id the deployment record does not have", (path, text) => {
    // The drift that actually happened: an address left behind by a previous
    // deploy, still sitting in a table looking authoritative.
    for (const id of new Set(text.match(/C[A-Z2-7]{55}/g) ?? [])) {
      expect(deployedIds, `${path} names ${id}, which no deployment record has`).toContain(id);
    }
  });

  it.each(DOCS)("%s leaks no secret key", (_path, text) => {
    expect(text).not.toMatch(/\bS[A-Z2-7]{55}\b/);
  });
});

describe("the docs point at things that exist", () => {
  /** Relative markdown link targets: `[text](./path)` and `[text](path/file)`. */
  function localLinks(text: string): string[] {
    const found: string[] = [];
    for (const [, target] of text.matchAll(/\]\(([^)\s]+)\)/g)) {
      if (/^(https?:|mailto:|#)/.test(target)) continue;
      found.push(target.split("#")[0]);
    }
    return found.filter(Boolean);
  }

  it.each(DOCS)("%s links only to files that exist", (path, text) => {
    const base = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : ".";
    for (const target of localLinks(text)) {
      const resolved = join(root, base, target);
      expect(existsSync(resolved), `${path} links to ${target}, which does not exist`).toBe(true);
    }
  });

  /**
   * Backticked strings that look like a file in this repository.
   *
   * The dead `apps/dashboard/app/lib/wallet/kit.ts` citation was written this
   * way, not as a markdown link, so the link rule above sailed past it — and it
   * sat in the table that satisfies a submission requirement. Deliberately
   * narrow: a path shape with a known extension, and nothing that is obviously
   * not ours.
   */
  function citedPaths(text: string): string[] {
    const found = new Set<string>();
    for (const [, inner] of text.matchAll(/`([^`\n]+)`/g)) {
      const value = inner.trim();
      if (!/^[\w.-]+(\/[\w.-]+)+\.(ts|tsx|rs|mjs|mts|json|toml|md)$/.test(value)) continue;
      // Placeholders, dependencies, and the Stellar skill files — which are
      // cited by path on purpose and live at skills.stellar.org, not here.
      if (/^(node_modules|skills)\//.test(value)) continue;
      if (value.includes("<") || value.includes("*")) continue;
      found.add(value);
    }
    return [...found];
  }

  /**
   * Documents that describe the tree as it is now, and that a reader deciding
   * whether to trust this project actually opens.
   *
   * `PROJECT.md` and `V2-CUTOVER.md` are deliberately outside it. They are a
   * build plan and a migration log — records of what was intended and what was
   * done at a point in time, and a path in one of them is a historical fact
   * rather than a claim about the current tree. Correcting them to match today
   * would be rewriting the record to pass a test, which is the opposite of what
   * this file is for.
   */
  const CURRENT = new Set([
    "README.md",
    "AGENTS.md",
    "docs/ARCHITECTURE.md",
    "docs/ROADMAP.md",
    "docs/QUICKSTART.md",
    "docs/DELIVERABLES.md",
  ]);

  it.each(DOCS.filter(([path]) => CURRENT.has(path)))(
    "%s cites no path in this repo that is missing",
    (path, text) => {
      for (const cited of citedPaths(text)) {
        expect(
          existsSync(join(root, cited)),
          `${path} cites \`${cited}\`, which is not a file in this repository`,
        ).toBe(true);
      }
    },
  );

  it.each(DOCS)("%s claims no deployment record that is missing", (path, text) => {
    // ARCHITECTURE.md presented an anchor deployment as live before one existed.
    // A deployment named in prose has to be a deployment the package carries.
    for (const [, name] of text.matchAll(/deployments\/([a-z0-9-]+)\.json/g)) {
      expect(
        listNetworks(),
        `${path} names deployments/${name}.json, which is not a network @bound/sdk carries`,
      ).toContain(name);
    }
  });
});

describe("nothing unfinished ships invisibly", () => {
  it.each(DOCS)("%s carries no HTML-comment TODO", (path, text) => {
    // `<!-- TODO -->` renders as nothing on GitHub, so a section flagged this
    // way looks finished to every reader except the person who wrote it. If
    // something is unfinished it has to be unfinished *out loud*.
    const hidden = text.match(/<!--[^>]*\b(TODO|FIXME|XXX)\b[^>]*-->/gi) ?? [];
    expect(
      hidden,
      `${path} hides ${hidden.length} unfinished marker(s) in an HTML comment`,
    ).toEqual([]);
  });
});
