import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { afterAll, beforeAll, expect, it } from "vitest";

let directory;
let client;
let base;
let head;
let invalid;
const script = resolve("scripts/check-commits.mjs");

function git(cwd, args, input) {
  return execFileSync("git", args, {
    cwd,
    input,
    encoding: "utf8",
    env: {
      ...process.env,
      GIT_AUTHOR_NAME: "Test",
      GIT_AUTHOR_EMAIL: "test@example.com",
      GIT_COMMITTER_NAME: "Test",
      GIT_COMMITTER_EMAIL: "test@example.com",
    },
  }).trim();
}

beforeAll(() => {
  directory = mkdtempSync(join(tmpdir(), "emploidle-commits-"));
  const origin = join(directory, "origin");
  client = join(directory, "client");
  git(directory, ["init", "--bare", "--quiet", origin]);
  const tree = git(origin, ["hash-object", "-w", "-t", "tree", "--stdin"], "");
  const commit = (message, parents = []) =>
    git(origin, ["commit-tree", tree, ...parents], `${message}\n`);
  const initial = commit("feat: add application");
  base = commit("chore(release): 1.1.0", ["-p", initial]);
  head = commit("chore(release): 1.0.0", ["-p", initial]);
  invalid = commit("invalid message", ["-p", head]);
  git(origin, ["update-ref", "refs/heads/main", head]);
  git(origin, ["update-ref", "refs/heads/invalid", invalid]);
  git(directory, [
    "clone",
    "--quiet",
    "--no-local",
    "--no-checkout",
    "--branch",
    "main",
    origin,
    client,
  ]);
  for (const file of ["package.json", "commitlint.config.js"])
    copyFileSync(resolve(file), join(client, file));
  symlinkSync(
    resolve("node_modules"),
    join(client, "node_modules"),
    "junction",
  );
});

afterAll(() => {
  if (directory) rmSync(directory, { recursive: true, force: true });
});

function check(headSha, baseSha = base) {
  return spawnSync(process.execPath, [script], {
    cwd: client,
    encoding: "utf8",
    env: { ...process.env, BASE_SHA: baseSha, HEAD_SHA: headSha },
  });
}

it("validates a rewritten history whose previous tip is absent locally", () => {
  expect(
    spawnSync("git", ["cat-file", "-e", base], { cwd: client }).status,
  ).not.toBe(0);
  const result = check(head);
  expect(result.stderr).not.toContain("Invalid revision range");
  expect(result.status).toBe(0);
});

it("still rejects invalid commit messages", () => {
  const result = check(invalid);
  expect(result.status).not.toBe(0);
  expect(result.stdout + result.stderr).toContain("subject-empty");
});

it("fails when the previous tip cannot be fetched", () => {
  expect(check(head, "1".repeat(40)).status).not.toBe(0);
});
