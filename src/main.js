const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const toggle = document.getElementById("toggle");
const status = document.getElementById("status");
const configuration = document.getElementById("configuration");
const windowList = document.getElementById("windows");
const practice = document.getElementById("practice");
const error = document.getElementById("error");
const preferences = document.getElementById("preferences");
const preferencesForm = document.getElementById("preferences-form");
const scheduleStart = document.getElementById("schedule-start");
const scheduleFinish = document.getElementById("schedule-finish");
const autostart = document.getElementById("autostart");
let active = false;
let pending = true;
let activityStatus = "Idle";

function render() {
  toggle.dataset.active = String(active);
  toggle.textContent = active ? "Stop" : "Start";
  toggle.disabled = pending;
  configuration.disabled = active || pending;
  preferences.disabled = pending;
  status.textContent = activityStatus || "Idle";
}

function showError(cause) {
  error.textContent = String(cause);
}

function renderPreferences(saved) {
  scheduleStart.value = saved.schedule.start;
  scheduleFinish.value = saved.schedule.finish;
  autostart.checked = saved.autostart;
}

preferencesForm.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!preferencesForm.reportValidity()) return;
  perform(async () => {
    const validTime = /^(?:[01]\d|2[0-3]):[0-5]\d$/;
    if (
      !validTime.test(scheduleStart.value) ||
      !validTime.test(scheduleFinish.value)
    ) {
      throw new Error("Enter valid start and finish times.");
    }
    if (
      scheduleStart.value === scheduleFinish.value &&
      scheduleStart.value !== "00:00"
    ) {
      throw new Error("Start and finish must differ, or both be 00:00.");
    }
    if (!active) {
      await configureSelection(
        scheduleStart.value !== "00:00" || scheduleFinish.value !== "00:00",
        false,
      );
    }
    renderPreferences(
      await invoke("update_preferences", {
        preferences: {
          schedule: {
            start: scheduleStart.value,
            finish: scheduleFinish.value,
          },
          autostart: autostart.checked,
        },
      }),
    );
  });
});

function selectedTargets() {
  return [...windowList.querySelectorAll(".window-option")]
    .filter((row) => row.querySelector("input").checked)
    .map((row) => ({
      id: Number(row.dataset.id),
      context: row.querySelector("select").value,
    }));
}

async function configureSelection(requireTarget = true, openPractice = true) {
  const targets = selectedTargets();
  if (requireTarget && !targets.length && !practice.checked) {
    throw new Error(
      "Choose an activity window or enable the practice document.",
    );
  }
  if (practice.checked && openPractice) await invoke("open_practice");
  await invoke("configure", {
    configuration: { targets, practice: practice.checked },
  });
}

async function refreshWindows(targets = selectedTargets()) {
  const windows = await invoke("list_windows");
  const contexts = new Map(
    targets.map((target) => [target.id, target.context]),
  );
  windowList.replaceChildren();
  for (const window of windows) {
    const row = document.createElement("div");
    row.className = "window-option";
    row.dataset.id = String(window.id);
    const label = document.createElement("label");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = contexts.has(window.id);
    const title = document.createElement("span");
    title.textContent = window.title || window.application;
    title.title = `${window.application}: ${window.title}`;
    label.append(checkbox, title);
    const context = document.createElement("select");
    context.setAttribute("aria-label", `Activity for ${title.textContent}`);
    for (const value of ["reading", "preview"]) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = value === "reading" ? "Reading" : "Local preview";
      context.append(option);
    }
    context.value = contexts.get(window.id) || "reading";
    context.disabled = !checkbox.checked;
    checkbox.addEventListener("change", () => {
      context.disabled = !checkbox.checked;
    });
    const preview = document.createElement("button");
    preview.type = "button";
    preview.className = "secondary";
    preview.textContent = "Preview";
    preview.setAttribute("aria-label", `Preview ${title.textContent}`);
    preview.addEventListener("click", () =>
      perform(() => invoke("open_preview", { id: window.id })),
    );
    row.append(label, context, preview);
    windowList.append(row);
  }
  if (windows.length === 0) {
    const empty = document.createElement("p");
    empty.className = "hint";
    empty.textContent =
      "No available windows. Open an application and refresh, or use the practice document.";
    windowList.append(empty);
  }
}

async function perform(action) {
  if (pending) return;
  pending = true;
  error.textContent = "";
  render();
  try {
    await action();
  } catch (cause) {
    showError(cause);
  } finally {
    pending = false;
    render();
  }
}

toggle.addEventListener("click", () =>
  perform(async () => {
    const next = !active;
    if (next) {
      await configureSelection();
    }
    await invoke("set_active", { active: next });
    active = Boolean(await invoke("is_active"));
    activityStatus = await invoke("activity_status");
  }),
);

document
  .getElementById("refresh")
  .addEventListener("click", () => perform(() => refreshWindows()));
document
  .getElementById("open-practice")
  .addEventListener("click", () => perform(() => invoke("open_practice")));

(async () => {
  try {
    await listen("running-changed", (event) => {
      active = Boolean(event.payload);
      if (!active) activityStatus = "Idle";
      render();
    });
    await listen("activity-status", (event) => {
      activityStatus = String(event.payload);
      render();
    });
    document.getElementById("version").textContent =
      `v${await invoke("app_version")}`;
    const saved = await invoke("get_configuration");
    practice.checked = saved.practice;
    await refreshWindows(saved.targets);
    activityStatus = await invoke("activity_status");
    active = Boolean(await invoke("is_active"));
  } catch (cause) {
    showError(cause);
  }
  try {
    renderPreferences(await invoke("get_preferences"));
  } catch (cause) {
    showError(cause);
  } finally {
    pending = false;
    render();
  }
})();
