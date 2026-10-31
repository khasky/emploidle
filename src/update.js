(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const banner = document.getElementById("update-banner");
  const message = document.getElementById("update-message");
  const install = document.getElementById("update-install");
  const later = document.getElementById("update-later");
  let busy = false;

  later.addEventListener("click", () => {
    banner.hidden = true;
  });

  install.addEventListener("click", async () => {
    if (busy) return;
    busy = true;
    install.disabled = true;
    later.disabled = true;
    message.textContent = "Downloading update…";
    try {
      await invoke("install_update");
    } catch (error) {
      message.textContent = `Update failed: ${error}`;
      install.textContent = "Retry";
      install.disabled = false;
      later.disabled = false;
      busy = false;
    }
  });

  (async () => {
    try {
      await listen("update-progress", ({ payload }) => {
        if (!busy) return;
        message.textContent = payload.total
          ? `Downloading update… ${Math.min(100, Math.round((payload.downloaded / payload.total) * 100))}%`
          : "Downloading update…";
      });
      await listen("update-installing", () => {
        if (busy) message.textContent = "Installing update… Restarting Emploidle.";
      });
      const version = await invoke("check_update");
      if (version) {
        message.textContent = `Emploidle ${version} is available.`;
        banner.hidden = false;
      }
    } catch (error) {
      console.error("Update check failed:", error);
    }
  })();
})();
