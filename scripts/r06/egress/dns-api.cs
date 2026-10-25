// Fixed synthetic D0 probes. No hooks, resolver configuration, or service changes.
using System;
using System.Net;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
public static class AuraD0Dns {
    [StructLayout(LayoutKind.Sequential)] struct Server {
        public uint Type; public ulong Flags; public IntPtr Template;
        [MarshalAs(UnmanagedType.ByValArray, SizeConst=32)] public byte[] Address;
    }
    [StructLayout(LayoutKind.Sequential)] struct Request {
        public uint Version; public IntPtr Name; public ushort Type; public ulong Options;
        public IntPtr Servers; public uint Interface; public IntPtr Callback, Context;
        public int Required; public uint Network, Count; public IntPtr Custom;
    }
    [StructLayout(LayoutKind.Sequential)] struct Result {
        public uint Version; public int Status; public ulong Options; public IntPtr Records, Reserved;
    }
    [DllImport("dnsapi.dll", ExactSpelling=true)] static extern int DnsQueryEx(ref Request q, ref Result r, IntPtr cancel);
    [DllImport("dnsapi.dll", ExactSpelling=true)] static extern void DnsRecordListFree(IntPtr r, int kind);
    [DllImport("ws2_32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)] static extern int GetAddrInfoW(string name, string service, IntPtr hints, out IntPtr result);
    [DllImport("ws2_32.dll", ExactSpelling=true)] static extern void FreeAddrInfoW(IntPtr result);
    [DllImport("ws2_32.dll", ExactSpelling=true)] static extern int WSAStartup(ushort version, IntPtr data);
    [DllImport("ws2_32.dll", ExactSpelling=true)] static extern int WSACleanup();
    public static void Validate(string name) {
        if (!System.Text.RegularExpressions.Regex.IsMatch(name, "^[a-f0-9]{32}\\.r06\\.invalid$")) throw new ArgumentException("Synthetic query required");
    }
    static object Query(string name, string peer, ushort type) {
        Validate(name);
        if (type!=1 && type!=28) throw new ArgumentException("A/AAAA only");
        var result=new Result {Version=1};
        IntPtr serverMemory=IntPtr.Zero, queryName=Marshal.StringToHGlobalUni(name);
        try {
            if (!String.IsNullOrEmpty(peer)) {
                var endpoint=new IPEndPoint(IPAddress.Parse(peer),0).Serialize();
                var server=new Server {Type=1,Address=new byte[32]};
                for(int i=0;i<endpoint.Size;i++) server.Address[i]=endpoint[i];
                serverMemory=Marshal.AllocHGlobal(Marshal.SizeOf<Server>());
                Marshal.StructureToPtr(server,serverMemory,false);
            }
            var request=new Request {Version=3,Name=queryName,Type=type,Options=0x8|0x40|0x1000,Count=serverMemory==IntPtr.Zero?0u:1u,Custom=serverMemory};
            int status=DnsQueryEx(ref request,ref result,IntPtr.Zero);
            return new {api_status=status,result_status=result.Status,version=3,query_name=name,query_type=type,options=request.Options,server=peer,server_port=0,interface_index=0,custom_server_count=request.Count};
        }finally{
            if(result.Records!=IntPtr.Zero) DnsRecordListFree(result.Records,1);
            Marshal.FreeHGlobal(queryName); if(serverMemory!=IntPtr.Zero) Marshal.FreeHGlobal(serverMemory);
        }
    }
    public static Task<object> QueryAsync(string name,string peer,ushort type) {return Task.Run(()=>Query(name,peer,type));}
    public static Task<int> AddressAsync(string name) {
        Validate(name);
        return Task.Run(()=>{
            IntPtr data=Marshal.AllocHGlobal(512), result=IntPtr.Zero;
            bool started=false;
            try {int status=WSAStartup(0x202,data); if(status!=0)return status;started=true;return GetAddrInfoW(name,null,IntPtr.Zero,out result);}
            finally {if(result!=IntPtr.Zero)FreeAddrInfoW(result);if(started)WSACleanup();Marshal.FreeHGlobal(data);}
        });
    }
}
