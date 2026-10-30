import { execFileSync } from "node:child_process";
import { assetName } from "./lib/release-assets.mjs";

const { REPO: repo, TAG: tag } = process.env;
if (!repo || !tag) throw new Error("REPO and TAG are required");
const gh = (...args) => execFileSync("gh", args, { encoding: "utf8" });
const release = JSON.parse(
  gh("release", "view", tag, "--repo", repo, "--json", "databaseId"),
);
const assets = JSON.parse(
  gh(
    "api",
    `repos/${repo}/releases/${release.databaseId}/assets`,
    "--paginate",
    "--slurp",
  ),
).flat();
for (const asset of assets) {
  const name = assetName(asset.name, tag.replace(/^v/, ""));
  if (name === asset.name) continue;
  gh(
    "api",
    "--method",
    "PATCH",
    `repos/${repo}/releases/assets/${asset.id}`,
    "-f",
    `name=${name}`,
  );
  console.log(`${asset.name} -> ${name}`);
}
