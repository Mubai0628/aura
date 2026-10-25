# Fixed DNS observations use the Windows 11 API with an explicit peer. They do
# not change adapters, the DNS Client service, NRPT, hosts, or system DNS settings.
if(-not ('AuraR06DnsProbe' -as [type])){
Add-Type -TypeDefinition @'
using System;
using System.Net;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
public static class AuraR06DnsProbe {
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
        public uint Version; public int Status; public ulong Options;
        public IntPtr Records, Reserved;
    }
    [DllImport("dnsapi.dll", ExactSpelling=true)] static extern int DnsQueryEx(ref Request request, ref Result result, IntPtr cancel);
    [DllImport("dnsapi.dll", ExactSpelling=true)] static extern void DnsRecordListFree(IntPtr records, int type);
    public static int Query(string name, string address, int port) {
        if(!System.Text.RegularExpressions.Regex.IsMatch(name,"^[a-z0-9-]{1,63}\\.r06\\.invalid$")) throw new ArgumentException("Fixed synthetic DNS name required");
        if((port < 1024 && port != 0 && port != 53) || port > 65535) throw new ArgumentException("Invalid DNS test port");
        var endpoint=new IPEndPoint(IPAddress.Parse(address),port).Serialize();
        var server=new Server { Type=1, Address=new byte[32] };
        for(int i=0;i<endpoint.Size;i++) server.Address[i]=endpoint[i];
        var serverMemory=Marshal.AllocHGlobal(Marshal.SizeOf<Server>());
        var queryName=Marshal.StringToHGlobalUni(name);
        var result=new Result {Version=1};
        try {
            Marshal.StructureToPtr(server,serverMemory,false);
            var request=new Request {Version=3,Name=queryName,Type=1,Options=0x8|0x40|0x1000,Count=1,Custom=serverMemory};
            return DnsQueryEx(ref request,ref result,IntPtr.Zero);
        }finally{
            if(result.Records!=IntPtr.Zero) DnsRecordListFree(result.Records,1);
            Marshal.FreeHGlobal(queryName);Marshal.FreeHGlobal(serverMemory);
        }
    }
    public static Task<int> QueryAsync(string name,string address,int port) {return Task.Run(()=>Query(name,address,port));}
    public static byte[] NegativeReply(byte[] query) {
        if(query.Length<17 || query.Length>512 || query[2]!=1 || query[4]!=0 || query[5]!=1) throw new ArgumentException("Not a fixed single DNS question");
        int i=12;
        while(i<query.Length && query[i]!=0) {
            int n=query[i++];if(n>63 || i+n>=query.Length) throw new ArgumentException("Compressed or truncated DNS question");i+=n;
        }
        if(i+5>query.Length) throw new ArgumentException("Truncated DNS question");
        var reply=new byte[i+5];Array.Copy(query,reply,reply.Length);reply[2]=0x81;reply[3]=0x83;
        for(i=6;i<12;i++)reply[i]=0;
        return reply;
    }
}
'@
}
