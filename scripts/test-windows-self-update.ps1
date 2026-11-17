$ErrorActionPreference = 'Stop'
$source = (Resolve-Path 'dist/cangling-keeper-windows-x86_64.exe').Path
$expectedVersion = (Select-String -Path Cargo.toml -Pattern '^version = "([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
foreach ($rollback in @($false, $true)) {
    $work = Join-Path $env:RUNNER_TEMP ("keeper self update " + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path "$work/data" -Force | Out-Null
    $target = Join-Path $work 'keeper.exe'
    $staged = Join-Path $work 'new.exe'
    $backup = Join-Path $work 'previous.exe'
    $helper = Join-Path $work 'helper.exe'
    Copy-Item $source $target
    Copy-Item $source $helper
    # Malformed EXEs are rejected during download validation. Use a valid PE
    # that exits on Keeper's arguments to exercise installation-time rollback.
    if ($rollback) { Copy-Item "$env:SystemRoot/System32/where.exe" $staged } else { Copy-Item $source $staged }
    Set-Content -Path "$work/data/sentinel.txt" -Value 'keep user data'
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start(); $port = $listener.LocalEndpoint.Port; $listener.Stop()
    $base = "http://127.0.0.1:$port"
    $old = $null
    $updater = $null
    $newPid = $null
    try {
        $old = Start-Process -FilePath $target -ArgumentList @('--listen', "127.0.0.1:$port", '--data-dir', "`"$work/data`"", '--no-open') -PassThru
        $ready = $false
        for ($i = 0; $i -lt 100; $i++) {
            try { $health = Invoke-RestMethod "$base/api/health" -TimeoutSec 1; $ready = $true; break } catch { Start-Sleep -Milliseconds 200 }
        }
        if (!$ready) { throw 'Original program did not start' }
        $plan = @{
            pid = $old.Id; target = $target; staged = $staged; backup = $backup
            args = @('--listen', "127.0.0.1:$port", '--data-dir', "$work/data", '--no-open')
            cwd = $work; health_url = "$base/api/health"; latest = "v$expectedVersion"
            result = "$work/result.json"
        }
        $plan | ConvertTo-Json -Depth 5 | Set-Content -Path "$work/plan.json" -Encoding utf8NoBOM
        $updater = Start-Process -FilePath $helper -ArgumentList @('--apply-self-update', "`"$work/plan.json`"") -PassThru -RedirectStandardOutput "$work/helper.stdout.log" -RedirectStandardError "$work/helper.stderr.log"
        Start-Sleep -Milliseconds 300
        Stop-Process -Id $old.Id -Force
        $old.WaitForExit()
        $phase = $null
        for ($i = 0; $i -lt 150; $i++) {
            if (Test-Path "$work/result.json") {
                try {
                    $result = Get-Content "$work/result.json" -Raw | ConvertFrom-Json
                    $phase = $result.phase
                    if ($phase -in @('complete', 'rolled_back', 'error')) { break }
                } catch {}
            }
            Start-Sleep -Milliseconds 300
        }
        $expectedPhase = if ($rollback) { 'rolled_back' } else { 'complete' }
        if ($phase -ne $expectedPhase) {
            if (Test-Path "$work/result.json") { Get-Content "$work/result.json" -Raw | Write-Host }
            if (Test-Path "$work/data/app-update.log") { Get-Content "$work/data/app-update.log" -Raw | Write-Host }
            foreach ($log in @("$work/helper.stdout.log", "$work/helper.stderr.log")) { if (Test-Path $log) { Get-Content $log -Raw | Write-Host } }
            throw "Unexpected updater result: $phase (expected $expectedPhase)"
        }
        $ready = $false
        for ($i = 0; $i -lt 100; $i++) {
            try { $health = Invoke-RestMethod "$base/api/health" -TimeoutSec 1; $ready = $true; break } catch { Start-Sleep -Milliseconds 200 }
        }
        if (!$ready) { throw 'Restarted program did not become healthy' }
        $newPid = $health.pid
        if ($newPid -eq $old.Id -or $health.version -ne $expectedVersion) { throw 'Wrong restarted program identity' }
        if ((Get-Content "$work/data/sentinel.txt" -Raw).Trim() -ne 'keep user data') { throw 'User data changed' }
        if (!$rollback -and !(Test-Path $backup)) { throw 'Original program backup missing' }
        Write-Host "Windows self-update passed: $expectedPhase, PID $newPid, original data retained"
    } finally {
        if (!$newPid) { try { $newPid = (Invoke-RestMethod "$base/api/health" -TimeoutSec 1).pid } catch {} }
        if ($newPid) { Stop-Process -Id $newPid -Force -ErrorAction SilentlyContinue }
        if ($old) { Stop-Process -Id $old.Id -Force -ErrorAction SilentlyContinue }
        if ($updater) { $updater.WaitForExit(5000) | Out-Null; Stop-Process -Id $updater.Id -Force -ErrorAction SilentlyContinue }
        Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
    }
}
