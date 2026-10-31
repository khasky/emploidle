import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { expect, it, vi } from "vitest";

const source = readFileSync(
  new URL("../../src/main.js", import.meta.url),
  "utf8",
);

async function mainWindow(days = [true, true, true, true, true, true, true]) {
  const elements = new Map(
    [
      "toggle",
      "status",
      "error",
      "preferences",
      "preferences-form",
      "schedule-start",
      "schedule-finish",
      "schedule-mon",
      "schedule-tue",
      "schedule-wed",
      "schedule-thu",
      "schedule-fri",
      "schedule-sat",
      "schedule-sun",
      "autostart",
      "version",
    ].map((id) => [
      id,
      {
        disabled: true,
        textContent: "",
        dataset: {},
        value: "00:00",
        checked: false,
        reportValidity: () => true,
        addEventListener(name, handler) {
          this[name] = handler;
        },
      },
    ]),
  );
  const events = new Map();
  let active = false;
  let saved = {
    schedule: { start: "00:00", finish: "00:00", days },
    autostart: false,
  };
  const invoke = vi.fn(async (command, payload) => {
    if (command === "set_active") {
      active = payload.active;
      return;
    }
    if (command === "is_active") return active;
    if (command === "activity_status")
      return active ? "Reading open applications" : "Idle";
    if (command === "app_version") return "1.0.1";
    if (command === "get_preferences") return saved;
    if (command === "update_preferences") {
      saved = payload.preferences;
      return saved;
    }
    throw new Error(`Unexpected manual configuration command: ${command}`);
  });
  runInNewContext(source, {
    window: {
      __TAURI__: {
        core: { invoke },
        event: { listen: async (name, handler) => events.set(name, handler) },
      },
    },
    document: {
      getElementById: (id) => {
        if (!elements.has(id))
          throw new Error(`Unexpected manual configuration element: ${id}`);
        return elements.get(id);
      },
    },
  });
  await vi.waitFor(() => expect(elements.get("toggle").disabled).toBe(false));
  expect(elements.get("error").textContent).toBe("");
  return { elements, events, invoke };
}

it("starts automatically without choosing windows or opening a local document", async () => {
  const ui = await mainWindow();
  await ui.elements.get("toggle").click();
  expect(ui.invoke).toHaveBeenCalledWith("set_active", { active: true });
  expect(ui.elements.get("toggle").textContent).toBe("Stop");
  expect(ui.elements.get("error").textContent).toBe("");
  await ui.elements.get("toggle").click();
  expect(ui.invoke).toHaveBeenCalledWith("set_active", { active: false });
  expect(ui.elements.get("toggle").textContent).toBe("Start");
});

it("saves a schedule without a target selection", async () => {
  const ui = await mainWindow();
  ui.elements.get("schedule-start").value = "09:00";
  ui.elements.get("schedule-finish").value = "18:00";
  ui.elements.get("schedule-sat").checked = false;
  ui.elements.get("schedule-sun").checked = false;
  ui.elements.get("preferences-form").submit({ preventDefault() {} });
  await vi.waitFor(() =>
    expect(ui.invoke).toHaveBeenCalledWith("update_preferences", {
      preferences: {
        schedule: {
          start: "09:00",
          finish: "18:00",
          days: [true, true, true, true, true, false, false],
        },
        autostart: false,
      },
    }),
  );
  expect(ui.elements.get("error").textContent).toBe("");
});

it("loads saved weekdays and allows disabling every day", async () => {
  const ui = await mainWindow([false, true, false, false, true, false, true]);
  for (const [day, checked] of [
    ["mon", false],
    ["tue", true],
    ["wed", false],
    ["thu", false],
    ["fri", true],
    ["sat", false],
    ["sun", true],
  ]) {
    expect(ui.elements.get(`schedule-${day}`).checked).toBe(checked);
    ui.elements.get(`schedule-${day}`).checked = false;
  }
  ui.elements.get("preferences-form").submit({ preventDefault() {} });
  await vi.waitFor(() =>
    expect(ui.invoke).toHaveBeenCalledWith("update_preferences", {
      preferences: {
        schedule: {
          start: "00:00",
          finish: "00:00",
          days: [false, false, false, false, false, false, false],
        },
        autostart: false,
      },
    }),
  );
});

it("keeps Stop available when opening the main window pauses activity", async () => {
  const ui = await mainWindow();
  ui.events.get("running-changed")({ payload: true });
  ui.events.get("activity-status")({ payload: "Paused" });
  expect(ui.elements.get("status").textContent).toBe("Paused");
  expect(ui.elements.get("toggle").disabled).toBe(false);
  expect(ui.elements.get("toggle").textContent).toBe("Stop");
});
