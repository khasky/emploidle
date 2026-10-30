import { describe, expect, it } from "vitest";
import { versionErrors } from "./version-parity.mjs";

const versions = {
  packageVersion: "1.0.0",
  tauriVersion: "1.0.0",
  manifest: '[package]\nname = "emploidle"\nversion = "1.0.0"\n',
  lock: '[[package]]\nname = "emploidle"\nversion = "1.0.0"\n',
  tag: "v1.0.0",
};

describe("versionErrors", () => {
  it("accepts synchronized versions and their tag", () => {
    expect(versionErrors(versions)).toEqual([]);
  });
  it("identifies the file whose version differs", () => {
    expect(versionErrors({ ...versions, tauriVersion: "1.0.1" })).toEqual([
      "src-tauri/tauri.conf.json: 1.0.1; expected 1.0.0",
    ]);
  });
  it("rejects missing Rust versions and the wrong tag", () => {
    expect(versionErrors({ ...versions, manifest: "", tag: "v1.0.1" })).toEqual(
      [
        "src-tauri/Cargo.toml: missing; expected 1.0.0",
        "tag v1.0.1 does not match v1.0.0",
      ],
    );
  });
  it("rejects multiline package versions", () => {
    expect(versionErrors({ ...versions, packageVersion: "1.0.0\n" })[0]).toBe(
      "package.json needs a version without whitespace",
    );
  });
});
