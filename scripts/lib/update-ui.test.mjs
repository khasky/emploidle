import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { expect, it, vi } from "vitest";

const source = readFileSync(
  new URL("../../src/update.js", import.meta.url),
  "utf8",
);

async function updater(check = () => Promise.resolve("1.0.2")) {
  const elements = new Map();
  const events = new Map();
  const invoke = vi.fn((command) =>
    command === "check_update" ? check() : Promise.resolve(),
  );
  function element(id) {
    if (!elements.has(id)) {
      elements.set(id, {
        hidden: true,
        disabled: false,
        textContent: "",
        addEventListener(name, handler) {
          this[name] = handler;
        },
      });
    }
    return elements.get(id);
  }
  runInNewContext(source, {
    window: {
      __TAURI__: {
        core: { invoke },
        event: {
          listen: async (name, handler) => events.set(name, handler),
        },
      },
    },
    document: { getElementById: element },
    console: { error: vi.fn() },
  });
  await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("check_update"));
  await Promise.resolve();
  return { element, events, invoke };
}

it("offers the newer version and allows deferring without installing", async () => {
  const ui = await updater();
  expect(ui.element("update-banner").hidden).toBe(false);
  expect(ui.element("update-message").textContent).toContain("1.0.2");
  ui.element("update-later").click();
  expect(ui.element("update-banner").hidden).toBe(true);
  expect(ui.invoke).not.toHaveBeenCalledWith("install_update");
});

it.each([() => Promise.resolve(null), () => Promise.reject("offline")])(
  "keeps the banner hidden when no update can be offered",
  async (check) => {
    const ui = await updater(check);
    expect(ui.element("update-banner").hidden).toBe(true);
  },
);

it("blocks duplicate installs, shows progress, and allows retry after failure", async () => {
  const ui = await updater();
  let rejectInstall;
  ui.invoke.mockImplementation(
    () =>
      new Promise((_, reject) => {
        rejectInstall = reject;
      }),
  );
  const installing = ui.element("update-install").click();
  await ui.element("update-install").click();
  expect(
    ui.invoke.mock.calls.filter(([command]) => command === "install_update"),
  ).toHaveLength(1);
  expect(ui.element("update-later").disabled).toBe(true);
  ui.events.get("update-progress")({ payload: { downloaded: 50, total: 100 } });
  expect(ui.element("update-message").textContent).toContain("50%");
  ui.events.get("update-installing")();
  expect(ui.element("update-message").textContent).toContain("Installing");
  rejectInstall("Invalid signature");
  await installing;
  expect(ui.element("update-message").textContent).toContain(
    "Invalid signature",
  );
  expect(ui.element("update-install").disabled).toBe(false);
  expect(ui.element("update-later").disabled).toBe(false);
  ui.invoke.mockResolvedValue();
  await ui.element("update-install").click();
  expect(
    ui.invoke.mock.calls.filter(([command]) => command === "install_update"),
  ).toHaveLength(2);
});
