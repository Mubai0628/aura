using System;
using System.Runtime.InteropServices;
public static class AuraD0WinHttp {
    [DllImport("winhttp.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr WinHttpOpen(string agent,uint access,string proxy,string bypass,uint flags);
    [DllImport("winhttp.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr WinHttpConnect(IntPtr session,string host,ushort port,uint reserved);
    [DllImport("winhttp.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr WinHttpOpenRequest(IntPtr connect,string verb,string path,string version,string referer,IntPtr accepts,uint flags);
    [DllImport("winhttp.dll", SetLastError=true)] static extern bool WinHttpSetTimeouts(IntPtr session,int resolve,int connect,int send,int receive);
    [DllImport("winhttp.dll", SetLastError=true)] static extern bool WinHttpSetOption(IntPtr request,uint option,ref uint value,uint size);
    [DllImport("winhttp.dll", SetLastError=true)] static extern bool WinHttpSendRequest(IntPtr request,IntPtr headers,uint headersLength,IntPtr body,uint length,uint total,UIntPtr context);
    [DllImport("winhttp.dll", SetLastError=true)] static extern bool WinHttpReceiveResponse(IntPtr request,IntPtr reserved);
    [DllImport("winhttp.dll")] static extern bool WinHttpCloseHandle(IntPtr handle);
    public static object Request(string host) {
        if(!System.Text.RegularExpressions.Regex.IsMatch(host,"^[a-f0-9]{32}\\.r06\\.invalid$"))throw new ArgumentException("Synthetic host required");
        IntPtr session=IntPtr.Zero,connect=IntPtr.Zero,request=IntPtr.Zero;
        string stage="open";bool sent=false,received=false;int error=0;
        try {
            session=WinHttpOpen("Aura-R06-D0",4,null,null,0);
            if(session==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();
            if(!WinHttpSetTimeouts(session,2000,2000,2000,2000))throw new System.ComponentModel.Win32Exception();
            stage="connect";connect=WinHttpConnect(session,host,62006,0);
            if(connect==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();
            stage="request";request=WinHttpOpenRequest(connect,"GET","/r06-fixed",null,null,IntPtr.Zero,0);
            if(request==IntPtr.Zero)throw new System.ComponentModel.Win32Exception();
            uint high=2; // WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH: never automatic credentials.
            if(!WinHttpSetOption(request,77,ref high,4))throw new System.ComponentModel.Win32Exception();
            stage="send";sent=WinHttpSendRequest(request,IntPtr.Zero,0,IntPtr.Zero,0,0,UIntPtr.Zero);
            if(!sent)throw new System.ComponentModel.Win32Exception();
            stage="receive";received=WinHttpReceiveResponse(request,IntPtr.Zero);
            if(!received)throw new System.ComponentModel.Win32Exception();
        }catch(System.ComponentModel.Win32Exception ex){error=ex.NativeErrorCode;}
        finally {if(request!=IntPtr.Zero)WinHttpCloseHandle(request);if(connect!=IntPtr.Zero)WinHttpCloseHandle(connect);if(session!=IntPtr.Zero)WinHttpCloseHandle(session);}
        return new {host,port=62006,access_type="automatic_proxy",automatic_credentials=false,stage,sent,received,error};
    }
}
