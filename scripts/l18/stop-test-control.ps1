function Stop-AuraTestControl {
    param([Parameter(Mandatory=$true)][int]$TestProcessId)
    if (-not ('AuraStopTestControl' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class AuraStopTestControl {
  delegate bool Visit(IntPtr window, IntPtr data);
  [DllImport("user32.dll")] static extern bool EnumWindows(Visit visit, IntPtr data);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern int GetClassNameW(IntPtr window, StringBuilder text, int length);
  [DllImport("user32.dll")] static extern bool PostMessageW(IntPtr window, uint message, UIntPtr w, IntPtr l);
  public static void Stop(uint process) {
    EnumWindows((window, data) => {
      uint owner; GetWindowThreadProcessId(window, out owner);
      if (owner == process) {
        var name = new StringBuilder(256);
        GetClassNameW(window, name, name.Capacity);
        if (name.ToString() == "AuraDesktopGuardBanner") PostMessageW(window, 0x0111, new UIntPtr(1), IntPtr.Zero);
      }
      return true;
    }, IntPtr.Zero);
  }
}
'@
    }
    [AuraStopTestControl]::Stop([uint32]$TestProcessId)
}
