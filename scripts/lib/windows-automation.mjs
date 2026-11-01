import { execFileSync, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { createInterface } from "node:readline";

const bootstrap =
  "[Console]::InputEncoding = [Text.UTF8Encoding]::new($false); & ([scriptblock]::Create(([Console]::In.ReadLine() | ConvertFrom-Json)))";
const scriptSource = () =>
  ["vscode.ps1", "browser.ps1", "accessibility.ps1"]
    .map((name) => readFileSync(`src-tauri/src/desktop/${name}`, "utf8"))
    .join("\n");

export const inputTick = () =>
  Number(
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        `
      Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class NativeInput {
    [StructLayout(LayoutKind.Sequential)] private struct State { public uint Size; public uint Tick; }
    [DllImport("user32.dll")] private static extern bool GetLastInputInfo(ref State state);
    public static uint Tick() { var state = new State { Size = 8 }; if (!GetLastInputInfo(ref state)) throw new Exception("Cannot read input tick"); return state.Tick; }
}
'@
      [NativeInput]::Tick()
    `,
      ],
      { encoding: "utf8", windowsHide: true },
    ),
  );

export function automationRequest(handle, operation, acknowledge = true) {
  const script = scriptSource();
  return JSON.parse(
    execFileSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-Command", bootstrap],
      {
        input: `${JSON.stringify(script)}\n${JSON.stringify({ ...handle, input_tick: inputTick(), ...operation })}\n${acknowledge ? "commit\n" : ""}`,
        encoding: "utf8",
        windowsHide: true,
        timeout: 3_000,
      },
    ),
  );
}

export async function readPageRequest(
  handle,
  address,
  selectText,
  { acknowledge = true, cancel = () => false } = {},
) {
  const child = spawn(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-Command", bootstrap],
    { windowsHide: true },
  );
  const lines = createInterface({ input: child.stdout });
  const exited = new Promise((accept, reject) => {
    child.once("exit", accept);
    child.once("error", reject);
  });
  let timeout;
  let cancellation;
  try {
    const request = {
      ...handle,
      input_tick: inputTick(),
      operation: "read_page",
      address,
      select_text: selectText,
    };
    return await new Promise((accept, reject) => {
      child.once("error", reject);
      child.stdin.on("error", reject);
      child.stderr.on("data", (data) => reject(new Error(data.toString())));
      let received = false;
      child.once("exit", () => {
        if (!received)
          reject(new Error("Reading helper exited without its result"));
      });
      lines.on("line", (line) => {
        try {
          const response = JSON.parse(line);
          if (response.recovery) {
            if (acknowledge) child.stdin.write("commit\n");
            else child.stdin.end();
          } else {
            received = true;
            accept(response);
          }
        } catch (error) {
          reject(error);
        }
      });
      cancellation = setInterval(() => {
        if (cancel()) {
          child.stdin.write("cancel\n");
          clearInterval(cancellation);
        }
      }, 30);
      timeout = setTimeout(
        () =>
          reject(new Error("Browser reading exceeded its 25-second budget")),
        25_000,
      );
      child.stdin.write(
        `${JSON.stringify(scriptSource())}\n${JSON.stringify(request)}\n`,
      );
    });
  } finally {
    clearTimeout(timeout);
    clearInterval(cancellation);
    child.stdin.end();
    await exited;
    lines.close();
  }
}

export function openPreparedEditor(handle, session, acknowledge = true) {
  execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName System.Windows.Forms
$root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]${handle.window})
$bounds = $root.Current.BoundingRectangle
[Windows.Forms.Cursor]::Position = [Drawing.Point]::new([int]($bounds.Left + $bounds.Width * 0.75), [int]($bounds.Top + $bounds.Height * 0.5))
`,
    ],
    { windowsHide: true },
  );
  nativeMouse(handle, session.file.bounds, { button: "move" });
  const target = automationRequest(handle, {
    operation: "editor_file_target",
    session,
  });
  if (!target.ok) throw new Error(target.error);
  nativeMouse(handle, target.data);
  return automationRequest(
    handle,
    { operation: "capture_editor", session },
    acknowledge,
  );
}

export function scrollEditorFile(handle, session, amount) {
  const target = automationRequest(handle, {
    operation: "editor_viewport",
    session,
  });
  if (!target.ok) throw new Error(target.error);
  nativeMouse(handle, target.data, { amount });
  return target.data;
}

export function clickEditorPoint(handle, point) {
  nativeMouse(handle, { ...point, width: 0, height: 0 }, { button: "left" });
}

function nativeMouse(handle, target, { amount = 0, button = "middle" } = {}) {
  const { x, y, width, height } = target;
  execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OwnedEditorMouse {
 [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern int GetSystemMetrics(int index);
 [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
 public static void Send(IntPtr window, int x, int y, int amount, uint down, uint up) {
  SetThreadDpiAwarenessContext(new IntPtr(-4));
  if(GetForegroundWindow()!=window) throw new Exception("The owned editor lost focus");
  for(int key=1;key<256;key++) if(GetAsyncKeyState(key)<0) throw new Exception("Input interrupted the owned file click");
  uint nx=(uint)Math.Round((x-GetSystemMetrics(76))*65535.0/(GetSystemMetrics(78)-1));
  uint ny=(uint)Math.Round((y-GetSystemMetrics(77))*65535.0/(GetSystemMetrics(79)-1));
  var marker=new UIntPtr(0x454d504c);
  mouse_event(0xc001,nx,ny,0,marker);
  if(amount==0 && down!=0) {mouse_event(down,0,0,0,marker);mouse_event(up,0,0,0,marker);}
  else if(amount!=0) mouse_event(0x800,0,0,unchecked((uint)(-120*amount)),marker);
 }
}
'@
[OwnedEditorMouse]::Send([IntPtr]${handle.window}, ${Math.round(x + width / 2)}, ${Math.round(y + height / 2)}, ${amount}, ${button === "move" ? 0 : button === "left" ? 2 : 0x20}, ${button === "move" ? 0 : button === "left" ? 4 : 0x40})
`,
    ],
    { encoding: "utf8", windowsHide: true },
  );
}

export function focusWindow(handle) {
  const focused = execFileSync(
    "powershell.exe",
    [
      "-NoProfile",
      "-NonInteractive",
      "-Command",
      `
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OwnedWindowFocus {
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, IntPtr unused);
 [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint a,uint b,bool attach);
 [DllImport("user32.dll")] private static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] private static extern bool BringWindowToTop(IntPtr window);
 [DllImport("user32.dll")] private static extern short GetAsyncKeyState(int key);
 public static bool Focus(IntPtr window) {
  if(GetForegroundWindow()==window || SetForegroundWindow(window)) return true;
  for(int key=1;key<256;key++) if(GetAsyncKeyState(key)<0) return false;
  uint current=GetCurrentThreadId(), foreground=GetWindowThreadProcessId(GetForegroundWindow(),IntPtr.Zero), target=GetWindowThreadProcessId(window,IntPtr.Zero);
  bool attachedForeground=foreground!=0&&foreground!=current&&AttachThreadInput(current,foreground,true);
  bool attachedTarget=target!=0&&target!=current&&target!=foreground&&AttachThreadInput(current,target,true);
  try { BringWindowToTop(window); SetForegroundWindow(window); return GetForegroundWindow()==window; }
  finally { if(attachedTarget) AttachThreadInput(current,target,false); if(attachedForeground) AttachThreadInput(current,foreground,false); }
 }
}
'@
[OwnedWindowFocus]::Focus([IntPtr]${handle.window})
`,
    ],
    { encoding: "utf8", windowsHide: true },
  );
  if (focused.trim() !== "True")
    throw new Error("The owned verification window could not be focused");
}
