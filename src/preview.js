const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const previewText = document.getElementById("preview-document");
const previewSource = document.getElementById("preview-source");

function renderPreview(snapshot) {
  previewText.value = snapshot.text;
  previewText.scrollTop = 0;
  previewText.scrollLeft = 0;
  previewSource.textContent = [snapshot.kind, snapshot.domain, snapshot.title]
    .filter(Boolean)
    .join(" · ");
  previewSource.title = previewSource.textContent;
}

function focusPreview() {
  previewText.focus({ preventScroll: true });
}

window.addEventListener("focus", focusPreview);
document.addEventListener("click", (event) => {
  if (!event.target.closest("textarea, button, input, select, a"))
    focusPreview();
});

(async () => {
  try {
    await listen("preview-changed", (event) => renderPreview(event.payload));
    renderPreview(await invoke("get_preview"));
    focusPreview();
  } catch (cause) {
    previewSource.textContent = String(cause);
  }
})();
