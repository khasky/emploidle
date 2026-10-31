const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const toggle = document.getElementById("toggle");
const status = document.getElementById("status");
const error = document.getElementById("error");
const preferences = document.getElementById("preferences");
const preferencesForm = document.getElementById("preferences-form");
const scheduleStart = document.getElementById("schedule-start");
const scheduleFinish = document.getElementById("schedule-finish");
const scheduleDays = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
  .map((day) => document.getElementById(`schedule-${day}`));
const autostart = document.getElementById("autostart");
let active = false;
let pending = true;
let activityStatus = "Idle";

function render() {
  toggle.dataset.active = String(active);
  toggle.textContent = active ? "Stop" : "Start";
  toggle.disabled = pending;
  preferences.disabled = pending;
  status.textContent = activityStatus || "Idle";
}

function renderPreferences(saved) {
  scheduleStart.value = saved.schedule.start;
  scheduleFinish.value = saved.schedule.finish;
  scheduleDays.forEach((checkbox, index) => {
    checkbox.checked = saved.schedule.days[index];
  });
  autostart.checked = saved.autostart;
}

async function perform(action) {
  if (pending) return;
  pending = true;
  error.textContent = "";
  render();
  try {
    await action();
  } catch (cause) {
    error.textContent = String(cause);
  } finally {
    pending = false;
    render();
  }
}

preferencesForm.addEventListener("submit", (event) => {
  event.preventDefault();
  if (!preferencesForm.reportValidity()) return;
  perform(async () => {
    renderPreferences(await invoke("update_preferences", {
      preferences: {
        schedule: {
          start: scheduleStart.value,
          finish: scheduleFinish.value,
          days: scheduleDays.map((checkbox) => checkbox.checked),
        },
        autostart: autostart.checked,
      },
    }));
  });
});

toggle.addEventListener("click", () => perform(async () => {
  await invoke("set_active", { active: !active });
  active = Boolean(await invoke("is_active"));
  activityStatus = await invoke("activity_status");
}));

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
    document.getElementById("version").textContent = `v${await invoke("app_version")}`;
    activityStatus = await invoke("activity_status");
    active = Boolean(await invoke("is_active"));
    renderPreferences(await invoke("get_preferences"));
  } catch (cause) {
    error.textContent = String(cause);
  } finally {
    pending = false;
    render();
  }
})();
