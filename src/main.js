const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const btn = document.getElementById("toggle");
const status = document.getElementById("status");

function render(isActive) {
  btn.dataset.active = String(isActive);
  btn.textContent = isActive ? "Stop" : "Start";
  status.textContent = isActive ? "Simulating activity…" : "Idle";
}

btn.addEventListener("click", async () => {
  const next = btn.dataset.active !== "true";
  render(next);
  await invoke("set_active", { active: next });
});

// Keep the UI in sync when Start/Stop is toggled from the tray menu.
listen("running-changed", (event) => render(Boolean(event.payload)));

(async () => {
  try {
    document.getElementById("version").textContent = `v${await invoke("app_version")}`;
    render(Boolean(await invoke("is_active")));
  } catch {
    render(false);
  }
})();
