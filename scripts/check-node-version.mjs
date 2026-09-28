import { readFileSync } from "node:fs";

const expected = readFileSync(new URL("../.node-version", import.meta.url), "utf8").trim();
const actual = process.versions.node;

if (actual !== expected) {
  process.stderr.write(
    `Sonux requires Node.js ${expected}; found ${actual}. Activate the version recorded in .nvmrc or .node-version and try again.\n`,
  );
  process.exit(1);
}
