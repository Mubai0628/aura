// Read-only token metadata for the caller and an explicitly identified sender.
using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Principal;
public static class AuraD0Token {
    [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,int pid);
    [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
    [DllImport("advapi32.dll",SetLastError=true)] static extern bool GetTokenInformation(IntPtr token,int kind,IntPtr data,int size,out int needed);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    static IntPtr Read(IntPtr token,int kind) {
        int size;GetTokenInformation(token,kind,IntPtr.Zero,0,out size);
        if(size<1 || size>65536)throw new Win32Exception(Marshal.GetLastWin32Error());
        var data=Marshal.AllocHGlobal(size);
        if(!GetTokenInformation(token,kind,data,size,out size)){int e=Marshal.GetLastWin32Error();Marshal.FreeHGlobal(data);throw new Win32Exception(e);}
        return data;
    }
    static string Sid(IntPtr ptr){return new SecurityIdentifier(ptr).Value;}
    static object[] Groups(IntPtr token,int kind) {
        var data=Read(token,kind);
        try {
            int count=Marshal.ReadInt32(data);if(count<0 || count>1024)throw new InvalidOperationException("Token group count");
            var result=new List<object>();int offset=IntPtr.Size==8?8:4,stride=IntPtr.Size==8?16:8;
            for(int i=0;i<count;i++){var item=IntPtr.Add(data,offset+i*stride);result.Add(new {sid=Sid(Marshal.ReadIntPtr(item)),attributes=unchecked((uint)Marshal.ReadInt32(item,IntPtr.Size))});}
            return result.ToArray();
        }finally{Marshal.FreeHGlobal(data);}
    }
    static object Snapshot(IntPtr token,int pid,string started) {
        IntPtr user=IntPtr.Zero,integrity=IntPtr.Zero,stats=IntPtr.Zero;
        try {
            user=Read(token,1);integrity=Read(token,25);stats=Read(token,10);
            return new {pid,process_started=started,user_sid=Sid(Marshal.ReadIntPtr(user)),
                groups=Groups(token,2),restricted_sids=Groups(token,11),logon_groups=Groups(token,28),
                integrity_sid=Sid(Marshal.ReadIntPtr(integrity)),token_type=Marshal.ReadInt32(stats,24),
                impersonation_level=Marshal.ReadInt32(stats,28),metadata_complete=true};
        }finally{Marshal.FreeHGlobal(user);Marshal.FreeHGlobal(integrity);Marshal.FreeHGlobal(stats);}
    }
    public static object Current() {
        using(var identity=WindowsIdentity.GetCurrent())using(var process=Process.GetCurrentProcess())
            return Snapshot(identity.AccessToken.DangerousGetHandle(),process.Id,process.StartTime.ToUniversalTime().ToString("O"));
    }
    public static object Sender(int pid) {
        IntPtr process=OpenProcess(0x1000,false,pid),token=IntPtr.Zero;
        if(process==IntPtr.Zero)throw new Win32Exception();
        try {
            if(!OpenProcessToken(process,8,out token))throw new Win32Exception();
            using(var info=Process.GetProcessById(pid))return Snapshot(token,pid,info.StartTime.ToUniversalTime().ToString("O"));
        }finally{if(token!=IntPtr.Zero)CloseHandle(token);CloseHandle(process);}
    }
}
