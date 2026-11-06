param(
    [Parameter(Mandatory=$true)][ValidatePattern('^plugin-setting:l18-credential-[a-f0-9]{12}/token$')][string]$Owner,
    [Parameter(Mandatory=$true)][ValidatePattern('^Aura/mcp/secret/[A-Za-z0-9_:-]{1,128}$')][string]$Target,
    [ValidateSet('inspect','cleanup')][string]$Action = 'inspect'
)
$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class AuraPluginCredentialProbe {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Credential {
    public uint Flags, Type; public string Target, Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
    public uint BlobSize; public IntPtr Blob; public uint Persist, AttributeCount;
    public IntPtr Attributes; public string Alias, User;
  }
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Attribute {
    public string Keyword; public uint Flags, ValueSize; public IntPtr Value;
  }
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CredReadW(string target, uint type, uint flags, out IntPtr item);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CredDeleteW(string target, uint type, uint flags);
  [DllImport("advapi32.dll")] static extern void CredFree(IntPtr value);
  public static bool ExistsOwned(string target, string owner) {
    IntPtr item;
    if (!CredReadW(target, 1, 0, out item)) {
      int error = Marshal.GetLastWin32Error();
      if (error == 1168) return false;
      throw new Exception("Test credential metadata read failed: " + error);
    }
    try {
      var value = Marshal.PtrToStructure<Credential>(item);
      for (int j=0; j<value.AttributeCount; j++) {
          var attribute = Marshal.PtrToStructure<Attribute>(IntPtr.Add(value.Attributes, j*Marshal.SizeOf<Attribute>()));
          if (attribute.Keyword != "aura.server" || attribute.ValueSize > 8192 || attribute.ValueSize % 2 != 0) continue;
          var declaredOwner = Marshal.PtrToStringUni(attribute.Value, (int)attribute.ValueSize/2);
          if (declaredOwner == owner && value.Target == target) return true;
      }
      throw new Exception("Test credential owner mismatch; record retained");
    } finally { CredFree(item); }
  }
  public static void DeleteOwned(string target, string owner) {
    if (ExistsOwned(target, owner)) {
      if (!CredDeleteW(target, 1, 0) && Marshal.GetLastWin32Error() != 1168) throw new Exception("Test credential deletion failed");
    }
    if (ExistsOwned(target, owner)) throw new Exception("Test credential remains after cleanup");
  }
}
'@
if ($Action -eq 'cleanup') { [AuraPluginCredentialProbe]::DeleteOwned($Target, $Owner) }
@{ exists=[AuraPluginCredentialProbe]::ExistsOwned($Target, $Owner); target=$Target; owner=$Owner } | ConvertTo-Json -Compress
