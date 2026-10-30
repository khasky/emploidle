import { describe, expect, it } from "vitest";
import config from "../../commitlint.config.js";

const rules = config.plugins[0].rules;
describe("commitlint footer rules", () => {
  it("requires an explanation for a breaking header", () => {
    const header = "feat(ui)!: change tray controls";
    expect(rules["breaking-change-footer"]({ header, raw: header })[0]).toBe(
      false,
    );
    expect(
      rules["breaking-change-footer"]({
        header,
        raw: `${header}\n\nBREAKING CHANGE: the previous layout is removed.`,
      })[0],
    ).toBe(true);
  });
  it("requires the reverted commit's hash", () => {
    expect(
      rules["revert-refs-footer"]({
        type: "revert",
        raw: "revert(ui): restore controls",
      })[0],
    ).toBe(false);
    expect(
      rules["revert-refs-footer"]({
        type: "revert",
        raw: "revert(ui): restore controls\n\nRefs: 79a8186",
      })[0],
    ).toBe(true);
  });
  it("accepts ordinary changes without special footers", () => {
    expect(
      rules["breaking-change-footer"]({
        header: "fix(ui): restore controls",
      })[0],
    ).toBe(true);
    expect(rules["revert-refs-footer"]({ type: "fix" })[0]).toBe(true);
  });
});
