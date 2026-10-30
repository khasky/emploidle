const documentText = document.getElementById("practice-document");
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
let generation = 0;
let current = null;

function restore(transaction) {
  if (documentText.value !== transaction.expected) return;
  documentText.value = transaction.baseline;
  documentText.setSelectionRange(
    transaction.selectionStart,
    transaction.selectionEnd,
  );
  documentText.scrollTop = transaction.scrollTop;
  documentText.scrollLeft = transaction.scrollLeft;
}

function cancelPlayback(restoreText = true) {
  generation += 1;
  if (current && restoreText) restore(current);
  current = null;
}

function valid(transaction) {
  if (generation !== transaction.generation || current !== transaction)
    return false;
  if (documentText.value !== transaction.expected) {
    cancelPlayback(false);
    return false;
  }
  return true;
}

function replaceInserted(transaction, inserted) {
  if (!valid(transaction)) return false;
  documentText.setRangeText(
    inserted,
    transaction.start,
    transaction.start + transaction.inserted.length,
    "end",
  );
  transaction.inserted = inserted;
  transaction.expected =
    transaction.baseline.slice(0, transaction.start) +
    inserted +
    transaction.baseline.slice(transaction.start);
  return true;
}

async function pause(transaction, minimum, maximum) {
  await new Promise((resolve) =>
    setTimeout(resolve, minimum + Math.random() * (maximum - minimum)),
  );
  return valid(transaction);
}

async function play({ id, epoch, text }) {
  cancelPlayback();
  const token = generation;
  let transaction;
  try {
    const allowed = await invoke("practice_can_type", { id, epoch });
    if (!allowed || token !== generation) return;
    transaction = {
      generation: token,
      baseline: documentText.value,
      expected: documentText.value,
      selectionStart: documentText.selectionStart,
      selectionEnd: documentText.selectionEnd,
      scrollTop: documentText.scrollTop,
      scrollLeft: documentText.scrollLeft,
      start: documentText.selectionEnd,
      inserted: "",
    };
    current = transaction;
    focusDocument();
    const normalizedText = text.replace(/\r\n?/g, "\n");
    const segments =
      typeof Intl.Segmenter === "function"
        ? [
            ...new Intl.Segmenter(undefined, {
              granularity: "grapheme",
            }).segment(normalizedText),
          ].map((part) => part.segment)
        : Array.from(normalizedText);
    for (const segment of segments) {
      if (!valid(transaction)) return;
      if (Math.random() < 0.025) {
        if (!replaceInserted(transaction, transaction.inserted + "e")) return;
        if (!(await pause(transaction, 130, 270))) return;
        if (!replaceInserted(transaction, transaction.inserted.slice(0, -1)))
          return;
      }
      if (!replaceInserted(transaction, transaction.inserted + segment)) return;
      if (!(await pause(transaction, segment === " " ? 100 : 45, 170))) return;
    }
    if (!(await pause(transaction, 500, 1100))) return;
    for (const segment of segments.reverse()) {
      if (
        !replaceInserted(
          transaction,
          transaction.inserted.slice(0, -segment.length),
        )
      )
        return;
      if (!(await pause(transaction, 35, 95))) return;
    }
  } catch {
    if (current === transaction) cancelPlayback();
  } finally {
    if (transaction && current === transaction) {
      restore(transaction);
      current = null;
    }
    await invoke("practice_done", { id }).catch(() => {});
  }
}

function focusDocument() {
  documentText.focus({ preventScroll: true });
}

window.addEventListener("focus", focusDocument);
window.addEventListener("blur", () => cancelPlayback());
for (const eventName of ["keydown", "pointerdown"]) {
  document.addEventListener(
    eventName,
    (event) => {
      if (event.isTrusted) cancelPlayback();
    },
    true,
  );
}
document.addEventListener("click", (event) => {
  if (!event.target.closest("textarea, button, input, select, a"))
    focusDocument();
});
focusDocument();

Promise.all([
  listen("practice-type", (event) => play(event.payload)),
  listen("practice-cancel", () => cancelPlayback()),
]).catch((cause) => {
  documentText.title = String(cause);
});
