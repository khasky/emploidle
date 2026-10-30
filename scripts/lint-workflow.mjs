import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const version = "1.7.12";
const targets = {
  "win32-x64": [
    "windows_amd64.zip",
    "6e7241b51e6817ea6a047693d8e6fed13b31819c9a0dd6c5a726e1592d22f6e9",
  ],
  "linux-x64": [
    "linux_amd64.tar.gz",
    "8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8",
  ],
  "darwin-x64": [
    "darwin_amd64.tar.gz",
    "5b44c3bc2255115c9b69e30efc0fecdf498fdb63c5d58e17084fd5f16324c644",
  ],
  "darwin-arm64": [
    "darwin_arm64.tar.gz",
    "aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f",
  ],
};
const target = `${process.platform}-${process.arch}`;
if (!targets[target])
  throw new Error(`Unsupported actionlint platform: ${target}`);
const [suffix, checksum] = targets[target];
const directory = join("src-tauri", "target", "actionlint", version, target);
const executable = join(
  directory,
  process.platform === "win32" ? "actionlint.exe" : "actionlint",
);
if (!existsSync(executable)) {
  const filename = `actionlint_${version}_${suffix}`;
  const response = await fetch(
    `https://github.com/rhysd/actionlint/releases/download/v${version}/${filename}`,
  );
  if (!response.ok)
    throw new Error(`actionlint download failed: ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (createHash("sha256").update(bytes).digest("hex") !== checksum) {
    throw new Error("actionlint checksum mismatch");
  }
  mkdirSync(directory, { recursive: true });
  const archive = join(directory, filename);
  writeFileSync(archive, bytes);
  execFileSync("tar", ["-xf", archive, "-C", directory], { stdio: "inherit" });
}
execFileSync(executable, [".github/workflows/build.yml"], { stdio: "inherit" });
