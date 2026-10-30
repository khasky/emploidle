import { execFileSync } from "node:child_process";

const head = process.env.HEAD_SHA;
const base = process.env.BASE_SHA;
if (!head || !/^[0-9a-f]{40}$/.test(head))
  throw new Error("HEAD_SHA must be a Git commit hash");
if (base && !/^[0-9a-f]{40}$/.test(base))
  throw new Error("BASE_SHA must be a Git commit hash");
const range = base && !/^0+$/.test(base) ? `${base}..${head}` : head;
const messages = execFileSync(
  "git",
  ["log", "--no-merges", "-z", "--format=%B", range],
  { encoding: "utf8" },
);
for (const message of messages.split("\0").filter((value) => value.trim())) {
  execFileSync(process.execPath, ["node_modules/@commitlint/cli/cli.js"], {
    input: `${message}\n`,
    stdio: ["pipe", "inherit", "inherit"],
  });
}
