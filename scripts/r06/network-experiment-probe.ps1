# Fixed diagnostic body embedded in the Spike; no caller-provided script.
param([string]$Peer,[string]$RunId)
$ErrorActionPreference='Stop'
. ./dns-probe-native.ps1
$direct=[ordered]@{ack=$false;error=$null}
$udp=[Net.Sockets.UdpClient]::new([Net.Sockets.AddressFamily]::InterNetworkV6)
try {
    $udp.Client.ReceiveTimeout=2000
    $token="$RunId/restricted/udp6"
    $bytes=[Text.Encoding]::ASCII.GetBytes($token)
    $null=$udp.Send($bytes,$bytes.Length,[Net.IPEndPoint]::new([Net.IPAddress]::Parse($Peer),62006))
    $remote=[Net.IPEndPoint]::new([Net.IPAddress]::IPv6Any,0)
    $direct.ack=[Text.Encoding]::ASCII.GetString($udp.Receive([ref]$remote)) -ceq "r06-ack:$token"
}catch{$direct.error=$_.Exception.GetBaseException().Message}finally{$udp.Dispose()}
$task=[AuraR06DnsProbe]::QueryAsync("$RunId-restricted.r06.invalid",$Peer,0)
$done=$task.Wait(6000)
[ordered]@{direct_udp=$direct;dns_completed=$done;dns_status=$(if($done){$task.GetAwaiter().GetResult()}else{$null});restricted_probe=$true}|ConvertTo-Json -Compress -Depth 4
