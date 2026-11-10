import { readFileSync } from "node:fs";
import { versionErrors } from "./lib/version-parity.mjs";

const read = (file) => readFileSync(file, "utf8");
const packageVersion = JSON.parse(read("package.json")).version;
const errors = versionErrors({
  packageVersion,
  manifest: read("Cargo.toml"),
  lock: read("Cargo.lock"),
  tag: process.env.RELEASE_TAG,
});
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
console.log(`All application versions agree: ${packageVersion}`);
