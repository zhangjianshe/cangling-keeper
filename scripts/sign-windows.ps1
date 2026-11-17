[CmdletBinding()]
param(
    [string]$ExePath = 'dist/cangling-keeper-windows-x86_64.exe',
    [string]$RootCertificate = "$PSScriptRoot/../docs/certificates/cangling-root-ca.cer"
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (!$env:KEEPER_WINDOWS_SIGNING_PFX -or !$env:KEEPER_WINDOWS_SIGNING_PASSWORD) {
    throw 'Configure KEEPER_WINDOWS_SIGNING_PFX and KEEPER_WINDOWS_SIGNING_PASSWORD repository secrets.'
}
$exe = (Resolve-Path -LiteralPath $ExePath).Path
$rootPath = (Resolve-Path -LiteralPath $RootCertificate).Path
$expectedRootHash = '89706BE8C7A9B1ACE3F16926EEADAB529BEF814DF666A03E3FC3B6308A1D8BB8'
if ((Get-FileHash -LiteralPath $rootPath -Algorithm SHA256).Hash -ne $expectedRootHash) {
    throw 'The public root certificate does not match the approved Cangling CA.'
}
$signTool = Get-ChildItem -Path "${env:ProgramFiles(x86)}/Windows Kits/10/bin/*/x64/signtool.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (!$signTool) { throw 'Windows SDK SignTool was not found.' }

$pfxPath = Join-Path ([IO.Path]::GetTempPath()) ("keeper-signing-" + [guid]::NewGuid() + '.pfx')
$signer = $null
$rootAdded = $false
$root = [Security.Cryptography.X509Certificates.X509Certificate2]::new($rootPath)
$rootStorePath = "Cert:/LocalMachine/Root/$($root.Thumbprint)"
try {
    [IO.File]::WriteAllBytes($pfxPath, [Convert]::FromBase64String($env:KEEPER_WINDOWS_SIGNING_PFX))
    $password = ConvertTo-SecureString $env:KEEPER_WINDOWS_SIGNING_PASSWORD -AsPlainText -Force
    Write-Host 'Importing the dedicated signing certificate.'
    $imported = @(Import-PfxCertificate -FilePath $pfxPath -Password $password -CertStoreLocation Cert:/CurrentUser/My)
    $signers = @($imported | Where-Object { $_.HasPrivateKey })
    if ($signers.Count -ne 1) { throw 'PFX must contain exactly one signing private key.' }
    $signer = $signers[0]
    if ($signer.Issuer -ne $root.Subject) { throw 'Signing certificate issuer does not match the Cangling root.' }
    if ($signer.NotAfter -le (Get-Date) -or $signer.NotBefore -gt (Get-Date)) { throw 'Signing certificate is outside its validity period.' }
    $ekuExtension = $signer.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' }
    $eku = @($ekuExtension.EnhancedKeyUsages | ForEach-Object { $_.Value })
    if ('1.3.6.1.5.5.7.3.3' -notin $eku) { throw 'Certificate is not valid for Code Signing.' }

    # Private-CA trust is installed only on this ephemeral verification runner.
    # It does not grant public Windows trust to customer machines.
    if (!(Test-Path $rootStorePath)) {
        Write-Host 'Installing temporary private-CA trust on the ephemeral runner.'
        Import-Certificate -FilePath $rootPath -CertStoreLocation Cert:/LocalMachine/Root | Out-Null
        $rootAdded = $true
    }
    Write-Host 'Signing the EXE and requesting an RFC3161 timestamp.'
    & $signTool.FullName sign /s My /sha1 $signer.Thumbprint /fd SHA256 `
        /tr http://timestamp.digicert.com /td SHA256 /d 'Cangling Keeper' $exe
    if ($LASTEXITCODE -ne 0) { throw "Authenticode signing/timestamping failed (exit $LASTEXITCODE)." }
    Write-Host 'Verifying the signed EXE.'
    & $signTool.FullName verify /pa /all /v /tw $exe
    if ($LASTEXITCODE -ne 0) { throw "Authenticode verification failed (exit $LASTEXITCODE)." }
    $signature = Get-AuthenticodeSignature -LiteralPath $exe
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Thumbprint -ne $signer.Thumbprint) {
        throw 'Authenticode signature is invalid or has an unexpected signer.'
    }
    if (!$signature.TimeStamperCertificate) { throw 'A trusted RFC3161 timestamp is required.' }
    Write-Host "Signed and verified: $($signer.Subject); expires $($signer.NotAfter.ToUniversalTime().ToString('u'))"
    Write-Host 'Verification uses the Cangling private CA. Customer machines must explicitly trust that CA.'
} finally {
    Remove-Item -LiteralPath $pfxPath -Force -ErrorAction SilentlyContinue
    if ($signer) { Remove-Item -Path "Cert:/CurrentUser/My/$($signer.Thumbprint)" -DeleteKey -Force -ErrorAction SilentlyContinue }
    if ($rootAdded) { Remove-Item -Path $rootStorePath -Force -ErrorAction SilentlyContinue }
    $env:KEEPER_WINDOWS_SIGNING_PFX = $null
    $env:KEEPER_WINDOWS_SIGNING_PASSWORD = $null
    $root.Dispose()
}
