import { describe, expect, it } from "vitest";
import cargoLock from "../cargo-lock-updater.cjs";
import cargoToml from "../cargo-toml-updater.cjs";

describe("Cargo version updaters", () => {
  it("changes the package version without changing dependencies", () => {
    const manifest =
      '[package]\nname = "emploidle"\nversion = "1.0.0"\n[dependencies]\nserde = { version = "1" }\n';
    const updated = cargoToml.writeVersion(manifest, "1.1.0");
    expect(cargoToml.readVersion(updated)).toBe("1.1.0");
    expect(updated).toContain('serde = { version = "1" }');
  });
  it("changes only this crate's locked version and preserves CRLF", () => {
    const lock =
      '[[package]]\r\nname = "other"\r\nversion = "1.0.0"\r\n\r\n[[package]]\r\nname = "emploidle"\r\nversion = "1.0.0"\r\n';
    expect(cargoLock.writeVersion(lock, "1.1.0")).toBe(
      '[[package]]\r\nname = "other"\r\nversion = "1.0.0"\r\n\r\n[[package]]\r\nname = "emploidle"\r\nversion = "1.1.0"\r\n',
    );
  });
  it("does not claim a dependency's version when the application is absent", () => {
    expect(
      cargoLock.readVersion('name = "other"\nversion = "1.0.0"'),
    ).toBeUndefined();
  });
});
