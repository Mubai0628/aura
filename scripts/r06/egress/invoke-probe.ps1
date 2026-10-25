# One bounded scenario per process; the caller must enforce its execution deadline.
param(
    [Parameter(Mandatory)][ValidateSet('dns_custom','dns_system','getaddrinfo','dotnet_dns','resolve_dnsname','nslookup','winhttp','udp','tcp')][string]$Scenario,
    [Parameter(Mandatory)][ValidatePattern('^[a-f0-9]{32}$')][string]$Canary,
    [Parameter(Mandatory)][string]$Peer,
    [ValidateSet(4,6)][int]$Family=4,
    [string]$GateDirectory,
    [switch]$GateFromStdin
)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
if($GateFromStdin){
    if($GateDirectory){throw 'Exactly one capture gate is allowed'}
    if([Console]::ReadLine() -cne 'start'){throw 'Fixed probe gate was not released'}
}
if($GateDirectory){
    $gate=[IO.Path]::GetFullPath($GateDirectory)
    if(-not (Test-Path -LiteralPath $gate -PathType Container)){throw 'Existing gate directory required'}
    # Waiting for UAC performs no probe and starts no trace. Keep it distinct
    # from the short execution deadline after the collector signals readiness.
    $deadline=[DateTime]::UtcNow.AddMinutes(10)
    while(-not (Test-Path -LiteralPath (Join-Path $gate 'go'))){if([DateTime]::UtcNow -gt $deadline){throw 'Capture gate timeout'};Start-Sleep -Milliseconds 100}
}
$address=[Net.IPAddress]::Parse($Peer)
if(($Family -eq 4) -ne ($address.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetwork)){throw 'Peer address family mismatch'}
$query="$Canary.r06.invalid"
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
Add-Type -TypeDefinition (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'token-api.cs') -Raw)
$token=[AuraD0Token]::Current()
$start=[DateTime]::UtcNow.ToString('O')
$report=[ordered]@{
    schema='r06-d0-request-private-v1';scenario=$Scenario;canary=$Canary
    caller_pid=$PID;caller_started=(Get-Process -Id $PID).StartTime.ToUniversalTime().ToString('O')
    caller_sid=$identity.User.Value;caller_groups=@($identity.Groups|ForEach-Object {$_.Value})
    caller_token=$token
    query_name=$query;peer=$Peer;family=$Family;started=$start
    request_initiated=$false;result=$null;error=$null;destination_reached='unknown'
    actual_sender='unknown';filter_hit='unknown';r06_complete=$false
}
try {
    switch($Scenario){
        {$_ -in @('dns_custom','dns_system','getaddrinfo')} {
            Add-Type -TypeDefinition (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'dns-api.cs') -Raw)
            $report.request_initiated=$true
            if($Scenario -eq 'getaddrinfo'){$report.parameters=@{address_family='unspecified'};$task=[AuraD0Dns]::AddressAsync($query)}else{
                $server=if($Scenario -eq 'dns_custom'){$Peer}else{''}
                $queryType=if($Family -eq 4){1}else{28}
                $report.parameters=@{version=3;query_type=$queryType;options=4168;interface_index=0;custom_server=$server;server_port=0}
                $task=[AuraD0Dns]::QueryAsync($query,$server,$queryType)
            }
            if(-not $task.Wait(10000)){throw 'DNS timeout; no isolation inference'}
            $report.result=$task.GetAwaiter().GetResult()
        }
        'dotnet_dns' {
            $report.parameters=@{address_family='unspecified'}
            $report.request_initiated=$true
            $task=[Net.Dns]::GetHostAddressesAsync($query)
            if(-not $task.Wait(10000)){throw 'System.Net.Dns timeout'}
            $report.result=@($task.GetAwaiter().GetResult()|ForEach-Object {$_.IPAddressToString})
        }
        'resolve_dnsname' {
            # Diagnostic-only explicit OS module, not a production runtime change.
            $module=Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell/v1.0/Modules/DnsClient/DnsClient.psd1'
            Import-Module -Name $module -ErrorAction Stop
            $command=Get-Command DnsClient\Resolve-DnsName -ErrorAction SilentlyContinue
            if(-not $command){throw 'Unsupported: Resolve-DnsName absent from fixed PS7'}
            $report.command_source=$command.Source;$report.request_initiated=$true
            $kind=if($Family -eq 4){'A'}else{'AAAA'}
            $report.module_path=$module;$report.module_sha256=(Get-FileHash -LiteralPath $module).Hash;$report.server=$Peer
            $report.result=@(DnsClient\Resolve-DnsName -Name $query -Server $Peer -Type $kind -DnsOnly -NoHostsFile -QuickTimeout -ErrorAction Stop|Select-Object Name,Type,IPAddress)
        }
        'nslookup' {
            $image=Join-Path ([Environment]::SystemDirectory) 'nslookup.exe'
            $report.image_sha256=(Get-FileHash -LiteralPath $image).Hash
            $kind=if($Family -eq 4){'-type=A'}else{'-type=AAAA'}
            $report.request_initiated=$true
            $report.result=@(& $image -timeout=2 -retry=1 $kind $query $Peer 2>&1|ForEach-Object {"$_"})
            $report.native_exit=$LASTEXITCODE
        }
        'winhttp' {
            Add-Type -TypeDefinition (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'winhttp-api.cs') -Raw)
            $report.request_initiated=$true
            $report.result=[AuraD0WinHttp]::Request($query)
        }
        'udp' {
            $udp=[Net.Sockets.UdpClient]::new($address.AddressFamily)
            try{
                $udp.Client.ReceiveTimeout=2000;$bytes=[Text.Encoding]::ASCII.GetBytes($Canary)
                $report.request_initiated=$true
                $sent=$udp.Send($bytes,$bytes.Length,[Net.IPEndPoint]::new($address,62006))
                $remote=[Net.IPEndPoint]::new($address,0)
                $reply=$udp.Receive([ref]$remote)
                $report.result=@{sent=$sent;reply=[Text.Encoding]::ASCII.GetString($reply);remote="$remote"}
            }finally{$udp.Dispose()}
        }
        'tcp' {
            $tcp=[Net.Sockets.TcpClient]::new($address.AddressFamily)
            try{
                $report.request_initiated=$true
                $task=$tcp.ConnectAsync($address,62006)
                if(-not $task.Wait(2000)){throw 'TCP connection timeout'}
                $null=$task.GetAwaiter().GetResult();$stream=$tcp.GetStream();$stream.ReadTimeout=2000
                $bytes=[Text.Encoding]::ASCII.GetBytes($Canary+"`n")
                $stream.Write($bytes,0,$bytes.Length);$buffer=[byte[]]::new(128)
                $count=$stream.Read($buffer,0,$buffer.Length)
                $report.result=@{reply=[Text.Encoding]::ASCII.GetString($buffer,0,$count)}
            }finally{$tcp.Dispose()}
        }
    }
}catch{$report.error=$_.Exception.GetBaseException().Message;$report.error_hresult=$_.Exception.GetBaseException().HResult}finally{
    $report.ended=[DateTime]::UtcNow.ToString('O')
    $report|ConvertTo-Json -Depth 8 -Compress
}
