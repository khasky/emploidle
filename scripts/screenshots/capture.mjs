import { mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright";
import mockTauri from "./tauri-mock.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const output = join(root, "screenshots");
const version = JSON.parse(
  await readFile(join(root, "package.json"), "utf8"),
).version;
const mainWindow = JSON.parse(
  await readFile(join(root, "src-tauri/tauri.conf.json"), "utf8"),
).app.windows[0];

await mkdir(output, { recursive: true });
const browser = await chromium.launch();
try {
  for (const theme of ["light", "dark"]) {
    const context = await browser.newContext({
      viewport: { width: mainWindow.width, height: mainWindow.height },
      deviceScaleFactor: 2,
      colorScheme: theme,
      reducedMotion: "reduce",
      locale: "en-US",
    });
    try {
      await context.addInitScript(mockTauri, { version });
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", (error) => errors.push(String(error)));
      page.on("console", (message) => {
        if (message.type() === "error") errors.push(message.text());
      });
      const open = async (state) => {
        const address = pathToFileURL(join(root, "src/index.html"));
        address.searchParams.set("screenshot", state);
        await page.goto(address.href);
        await page.waitForFunction(
          () =>
            !document.getElementById("toggle").disabled &&
            document.getElementById("schedule-start").value === "09:00",
        );
        if (await page.locator("#error").textContent())
          throw new Error("Main window reports an error");
      };
      const shot = (name, options = {}) =>
        page.screenshot({
          path: join(output, `${theme}-${name}.png`),
          animations: "disabled",
          fullPage: true,
          ...options,
        });

      await open("idle");
      if (await page.locator("#configuration, #windows, #practice").count())
        throw new Error("Manual window or practice settings are still visible");
      await shot("main");
      await page.locator("#preferences-form").screenshot({
        path: join(output, `${theme}-schedule.png`),
        animations: "disabled",
      });
      await open("paused");
      await page.waitForFunction(
        () => document.getElementById("status").textContent === "Paused",
      );
      await shot("paused");
      await open("update");
      await page.waitForFunction(
        () => !document.getElementById("update-banner").hidden,
      );
      await shot("update");
      if (errors.length) throw new Error(errors.join("\n"));
      console.log(`${theme}: main, schedule, paused and update captured`);
    } finally {
      await context.close();
    }
  }
} finally {
  await browser.close();
}
