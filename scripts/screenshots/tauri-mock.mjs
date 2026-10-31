export default function mockTauri({ version }) {
  const state =
    new URL(window.location.href).searchParams.get("screenshot") || "idle";
  const [major, minor, patch] = version.split(".");
  const answers = {
    app_version: version,
    is_active: state === "paused",
    activity_status: state === "paused" ? "Paused" : "Idle",
    get_preferences: {
      schedule: { start: "09:00", finish: "18:00" },
      autostart: false,
    },
    check_update:
      state === "update"
        ? `${major}.${minor}.${Number.parseInt(patch, 10) + 1}`
        : null,
  };
  window.__TAURI__ = {
    core: {
      invoke: async (command) => {
        if (!(command in answers)) {
          throw new Error(`Unexpected screenshot command: ${command}`);
        }
        return structuredClone(answers[command]);
      },
    },
    event: { listen: async () => () => {} },
  };
}
