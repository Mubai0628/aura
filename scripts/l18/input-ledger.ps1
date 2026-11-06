param([Parameter(Mandatory=$true)][string]$OutputPath, [Parameter(Mandatory=$true)][string]$StopPath)
$ErrorActionPreference = 'Stop'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
public static class AuraAcceptanceLedger {
  delegate IntPtr Hook(int code, UIntPtr message, IntPtr data);
  [StructLayout(LayoutKind.Sequential)] struct Last { public uint size, tick; }
  [StructLayout(LayoutKind.Sequential)] struct Message {
    public IntPtr window; public uint message; public UIntPtr w; public IntPtr l;
    public uint time; public int x,y; public uint extra;
  }
  [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref Last last);
  [DllImport("user32.dll")] static extern IntPtr SetWindowsHookExW(int kind, Hook hook, IntPtr module, uint thread);
  [DllImport("user32.dll")] static extern bool UnhookWindowsHookEx(IntPtr hook);
  [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr hook, int code, UIntPtr message, IntPtr data);
  [DllImport("user32.dll")] static extern int GetMessageW(out Message message, IntPtr window, uint low, uint high);
  [DllImport("user32.dll")] static extern bool PostThreadMessageW(uint thread, uint message, UIntPtr w, IntPtr l);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode)] static extern IntPtr GetModuleHandleW(string name);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  static readonly object Gate = new object();
  static readonly HashSet<uint> OwnTicks = new HashSet<uint>();
  static DateTime OwnerLast;
  static uint OtherTick;
  static int OtherEvents;
  static Hook Keyboard = (code, message, data) => Observe(false, code, message, data);
  static Hook Mouse = (code, message, data) => Observe(true, code, message, data);
  static IntPtr Observe(bool mouse, int code, UIntPtr message, IntPtr data) {
    if (code == 0) {
      // Read only timestamp and Aura marker: no keys, characters or pointer coordinates.
      uint tick = unchecked((uint)Marshal.ReadInt32(data, mouse ? 16 : 12));
      long marker = Marshal.ReadIntPtr(data, mouse ? 24 : 16).ToInt64();
      lock (Gate) {
        if (marker == 0x41555241 || marker == 0x54455354) OwnTicks.Add(tick);
        else { OwnerLast = DateTime.UtcNow; OtherTick = tick; OtherEvents++; }
      }
    }
    return CallNextHookEx(IntPtr.Zero, code, message, data);
  }
  public static void Run(string path, string stop) {
    if (!Environment.Is64BitProcess) throw new Exception("Ledger requires a 64-bit process");
    var last = new Last { size = (uint)Marshal.SizeOf<Last>() };
    if (!GetLastInputInfo(ref last)) throw new Exception("Cannot read idle timestamp");
    OwnerLast = DateTime.UtcNow.AddMilliseconds(-unchecked((uint)Environment.TickCount64 - last.tick));
    uint thread = GetCurrentThreadId();
    var keyboard = SetWindowsHookExW(13, Keyboard, GetModuleHandleW(null), 0);
    var mouse = SetWindowsHookExW(14, Mouse, GetModuleHandleW(null), 0);
    if (keyboard == IntPtr.Zero || mouse == IntPtr.Zero) {
      if (keyboard != IntPtr.Zero) UnhookWindowsHookEx(keyboard);
      if (mouse != IntPtr.Zero) UnhookWindowsHookEx(mouse);
      throw new Exception("Cannot install timestamp-only hooks");
    }
    bool writeFailed = false;
    int failedWrites = 0;
    using (var timer = new Timer(_ => {
      lock (Gate) {
        try {
          string json = "{\"healthy\":true,\"atUtc\":\"" + DateTime.UtcNow.ToString("o") +
            "\",\"ownerLastUtc\":\"" + OwnerLast.ToString("o") + "\",\"otherEvents\":" + OtherEvents +
            ",\"otherTick\":" + OtherTick + ",\"ownTicks\":[" + string.Join(",", OwnTicks) + "]}";
          File.WriteAllText(path + ".pending", json);
          File.Move(path + ".pending", path, true);
          failedWrites = 0;
        } catch (Exception error) when (error is IOException || error is UnauthorizedAccessException) {
          // A reader, such as a virus scan of the new file, can briefly prevent atomic replacement
          // on Windows, as a sharing error or as denied access; freshness still gates runs.
          writeFailed = ++failedWrites >= 20;
          if (failedWrites == 1 || writeFailed) Console.Error.WriteLine("Ledger metadata replacement: " + error.Message);
        } catch { writeFailed = true; }
        if (writeFailed || File.Exists(stop)) PostThreadMessageW(thread, 0x0012, UIntPtr.Zero, IntPtr.Zero);
      }
    }, null, 0, 250)) {
      try { Message message; while (GetMessageW(out message, IntPtr.Zero, 0, 0) > 0) {} }
      finally { UnhookWindowsHookEx(keyboard); UnhookWindowsHookEx(mouse); }
    }
    if (writeFailed) throw new Exception("Timestamp ledger write failed");
  }
}
'@
[AuraAcceptanceLedger]::Run($OutputPath, $StopPath)
