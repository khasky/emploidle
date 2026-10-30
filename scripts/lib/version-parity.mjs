import cargoLock from "../cargo-lock-updater.cjs";
import cargoToml from "../cargo-toml-updater.cjs";

export function versionErrors({
  packageVersion,
  tauriVersion,
  manifest,
  lock,
  tag,
}) {
  const versions = {
    "src-tauri/tauri.conf.json": tauriVersion,
    "src-tauri/Cargo.toml": cargoToml.readVersion(manifest),
    "src-tauri/Cargo.lock": cargoLock.readVersion(lock),
  };
  const errors = [];
  if (
    typeof packageVersion !== "string" ||
    !packageVersion ||
    /\s/.test(packageVersion)
  ) {
    errors.push("package.json needs a version without whitespace");
  }
  for (const [file, version] of Object.entries(versions)) {
    if (version !== packageVersion) {
      errors.push(
        `${file}: ${version ?? "missing"}; expected ${packageVersion}`,
      );
    }
  }
  if (tag && tag !== `v${packageVersion}`) {
    errors.push(`tag ${tag} does not match v${packageVersion}`);
  }
  return errors;
}
