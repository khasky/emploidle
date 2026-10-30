import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const source = readFileSync(
  new URL("../../src/practice.js", import.meta.url),
  "utf8",
);

function practice(allowed = () => Promise.resolve(true)) {
  const events = new Map();
  const input = new Map();
  const writes = [];
  const textarea = {
    value: "α😀 original\nnotes",
    selectionStart: 1,
    selectionEnd: 3,
    scrollTop: 10,
    scrollLeft: 4,
    focus() {},
    setSelectionRange(start, end) {
      this.selectionStart = start;
      this.selectionEnd = end;
    },
    setRangeText(text, start, end) {
      this.value = this.value.slice(0, start) + text + this.value.slice(end);
      this.setSelectionRange(start + text.length, start + text.length);
      writes.push(this.value);
    },
  };
  const invoke = vi.fn((command) =>
    command === "practice_can_type" ? allowed() : Promise.resolve(),
  );
  const math = Object.create(Math);
  math.random = () => 0.5;
  runInNewContext(source, {
    window: {
      __TAURI__: {
        core: { invoke },
        event: {
          listen(name, listener) {
            events.set(name, listener);
            return Promise.resolve(() => {});
          },
        },
      },
      addEventListener() {},
    },
    document: {
      getElementById: () => textarea,
      addEventListener: (name, listener) => input.set(name, listener),
    },
    setTimeout: (...args) => setTimeout(...args),
    Math: math,
  });
  return {
    textarea,
    writes,
    invoke,
    play: (text) =>
      events.get("practice-type")({ payload: { id: 1, epoch: 0, text } }),
    interrupt: () => input.get("keydown")({ isTrusted: true }),
  };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it("types Unicode graphemes and restores text, selection and scroll", async () => {
  const document = practice();
  const baseline = document.textarea.value;
  const playback = document.play("😀e\u0301\r\nnext");
  await vi.runAllTimersAsync();
  await playback;
  expect(document.writes).toContain(
    `${baseline.slice(0, 3)}😀e\u0301\nnext${baseline.slice(3)}`,
  );
  expect(document.textarea).toMatchObject({
    value: baseline,
    selectionStart: 1,
    selectionEnd: 3,
    scrollTop: 10,
    scrollLeft: 4,
  });
  expect(document.invoke).toHaveBeenLastCalledWith("practice_done", { id: 1 });
});

it("restores before trusted user input and preserves the subsequent edit", async () => {
  const document = practice();
  const baseline = document.textarea.value;
  const playback = document.play("temporary text");
  await vi.advanceTimersByTimeAsync(1);
  expect(document.textarea.value).not.toBe(baseline);
  document.interrupt();
  expect(document.textarea.value).toBe(baseline);
  document.textarea.value += " user edit";
  await vi.runAllTimersAsync();
  await playback;
  expect(document.textarea.value).toBe(`${baseline} user edit`);
});

it("discards approval that arrives after cancellation", async () => {
  let approve;
  const document = practice(
    () =>
      new Promise((resolve) => {
        approve = resolve;
      }),
  );
  const baseline = document.textarea.value;
  const playback = document.play("must not appear");
  document.interrupt();
  approve(true);
  await playback;
  expect(document.writes).toEqual([]);
  expect(document.textarea.value).toBe(baseline);
});
