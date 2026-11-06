function Get-AuraDesktopIdle {
    if (-not ('AuraTestDesktopIdle' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class AuraTestDesktopIdle {
  [StructLayout(LayoutKind.Sequential)] public struct Last { public uint size; public uint tick; }
  [DllImport("user32.dll")] static extern bool GetLastInputInfo(ref Last last);
  [DllImport("user32.dll")] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] static extern bool GetUserObjectInformation(IntPtr value, int index, StringBuilder text, uint length, out uint needed);
  [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr desktop);
  public static string Read() {
    var last = new Last { size = (uint)Marshal.SizeOf<Last>() };
    if (!GetLastInputInfo(ref last)) throw new Exception("GetLastInputInfo failed");
    var handle = OpenInputDesktop(0, false, 1);
    var text = new StringBuilder(256);
    uint needed;
    bool unlocked = handle != IntPtr.Zero && GetUserObjectInformation(handle, 2, text, 512, out needed) && text.ToString() == "Default";
    if (handle != IntPtr.Zero) CloseDesktop(handle);
    uint idle = unchecked((uint)Environment.TickCount64 - last.tick);
    return "{\"idleMs\":" + idle + ",\"lastInputTick\":" + last.tick + ",\"unlocked\":" + (unlocked ? "true" : "false") + "}";
  }
}

'@
    }
    [AuraTestDesktopIdle]::Read() | ConvertFrom-Json
}

if ($MyInvocation.InvocationName -ne '.') { Get-AuraDesktopIdle | ConvertTo-Json -Compress }
