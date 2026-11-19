$ErrorActionPreference = 'Stop'
$work = Join-Path $env:RUNNER_TEMP ("keeper-smoke-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
$portProbe = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
$portProbe.Start(); $portNumber = $portProbe.LocalEndpoint.Port; $portProbe.Stop()
$base = "http://127.0.0.1:$portNumber"
$process = $null
$rootPath = Join-Path $PSScriptRoot '../docs/certificates/cangling-root-ca.cer'
$root = [Security.Cryptography.X509Certificates.X509Certificate2]::new((Resolve-Path $rootPath).Path)
$storePath = "Cert:/LocalMachine/Root/$($root.Thumbprint)"
$wasInstalled = Test-Path $storePath
function Invoke-Keeper($command, $body) {
    Invoke-RestMethod -Uri "$base/api/invoke/$command" -Method Post -ContentType 'application/json' -Body ($body | ConvertTo-Json -Compress) -TimeoutSec 60
}
try {
    $process = Start-Process -FilePath (Resolve-Path 'dist/cangling-keeper-windows-x86_64.exe').Path `
        -ArgumentList @('--listen', "127.0.0.1:$portNumber", '--data-dir', "`"$work/data`"", '--no-open') `
        -PassThru -RedirectStandardOutput "$work/stdout.log" -RedirectStandardError "$work/stderr.log"
    $ready = $false
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        if ($process.HasExited) { throw 'Keeper exited during startup.' }
        try { $null = Invoke-RestMethod -Uri "$base/api/health" -TimeoutSec 1; $ready = $true; break } catch { Start-Sleep -Milliseconds 200 }
    }
    if (!$ready) { throw 'Keeper did not become ready.' }
    $status = Invoke-Keeper 'get_root_certificate_status' @{}
    if (!$status.ok -or !$status.result.supported -or $status.result.fingerprint -ne '0285F51BB6BEA21C74AD8C13320FFFB3C940438C54A4D8384A3FF2E78E6EBCD1') { throw 'Windows root certificate status check failed.' }
    # Hosted Windows runners already have administrative privileges. This tests
    # the real installer on an ephemeral machine; no customer trust store changes.
    $installed = Invoke-Keeper 'install_root_certificate' @{ confirmed = $true }
    if (!$installed.ok -or !$installed.result.installed) { throw 'Windows root certificate installation failed.' }
    $again = Invoke-Keeper 'install_root_certificate' @{ confirmed = $true }
    if (!$again.ok -or !$again.result.installed) { throw 'Repeated root installation was not idempotent.' }
    $sets = Invoke-Keeper 'list_software_sets' @{}
    $repo = @($sets.result | Where-Object { $_.name -eq 'cangling-repo' })
    if (!$sets.ok -or $repo.Count -ne 1 -or !(Test-Path -LiteralPath $repo[0].localPath -PathType Container) -or $repo[0].cloned) { throw 'First-visit repository directory was not created correctly.' }
    $folder = Join-Path $work 'directory with spaces 中文'
    New-Item -ItemType Directory -Path $folder | Out-Null
    $opened = Invoke-Keeper 'open_local_path' @{ path = $folder }
    if (!$opened.ok) { throw 'Windows Shell did not accept the directory-open request.' }
    $upstream = Join-Path $work 'upstream'
    git init --initial-branch=main $upstream | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the local Git fixture.' }
    [IO.File]::WriteAllText((Join-Path $upstream 'sample.txt'), 'fixture')
    git -C $upstream add sample.txt
    git -C $upstream -c user.name=KeeperTest -c user.email=test@example.invalid commit -m fixture | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not commit the local Git fixture.' }
    $url = ([Uri]$upstream).AbsoluteUri
    $null = Invoke-Keeper 'update_software_set' @{ setName='cangling-repo'; kind='git'; gitUrl=$url; gitUsername=''; gitToken=''; gitBranch='' }
    $firstSync = Invoke-Keeper 'sync_software_set' @{ setName='cangling-repo' }
    if (!$firstSync.ok -or !$firstSync.result.cloned) { throw 'Local Git fixture sync failed.' }
    $lockPath = Join-Path $firstSync.result.localPath '.git/index.lock'
    [IO.File]::WriteAllText($lockPath, '')
    (Get-Item -LiteralPath $lockPath).LastWriteTimeUtc = [DateTime]::UtcNow.AddMinutes(-2)
    $recovered = Invoke-Keeper 'sync_software_set' @{ setName='cangling-repo' }
    if (!$recovered.ok -or !$recovered.result.cloned -or (Test-Path -LiteralPath $lockPath)) { throw 'Verified stale Git lock recovery failed.' }
    Write-Host 'Windows stale Git lock recovery and local Git synchronization passed.'
    Write-Host 'Windows runtime: root installation, repeated installation, first-visit repository path and Unicode directory opening passed.'
} catch {
    foreach ($file in @("$work/stdout.log", "$work/stderr.log")) {
        if (Test-Path $file) { Get-Content $file | Select-Object -Last 30 | Write-Host }
    }
    throw
} finally {
    if ($process -and !$process.HasExited) { Stop-Process -Id $process.Id -Force }
    if (!$wasInstalled -and (Test-Path $storePath)) { Remove-Item $storePath -Force }
    $root.Dispose()
    Remove-Item -LiteralPath $work -Force -Recurse -ErrorAction SilentlyContinue
}
