import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

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
  const script = ["vscode.ps1", "accessibility.ps1"]
    .map((name) => readFileSync(`src-tauri/src/desktop/${name}`, "utf8"))
    .join("\n");
  return JSON.parse(
    execFileSync(
      "powershell.exe",
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        "[Console]::InputEncoding = [Text.UTF8Encoding]::new($false); & ([scriptblock]::Create(([Console]::In.ReadLine() | ConvertFrom-Json)))",
      ],
      {
        input: `${JSON.stringify(script)}\n${JSON.stringify({ ...handle, input_tick: inputTick(), ...operation })}\n${acknowledge ? "commit\n" : ""}`,
        encoding: "utf8",
        windowsHide: true,
        timeout: 3_000,
      },
    ),
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
