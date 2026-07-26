[CmdletBinding()]
param(
    [string]$DatabasePath = 'target/p4-02a-bybit-testnet-smoke.db'
)

$ErrorActionPreference = 'Stop'
$credentialPath = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.ironpilot\bybit-testnet-credential.clixml'
if (-not (Test-Path -LiteralPath $credentialPath)) {
    throw "Encrypted Bybit Testnet credential not found at $credentialPath"
}

function ConvertFrom-LocalSecureString {
    param([Parameter(Mandatory)][Security.SecureString]$Value)

    $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Value)
    try {
        [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
    }
    finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
    }
}

$credential = Import-Clixml -LiteralPath $credentialPath
if ($credential.Environment -ne 'BYBIT_TESTNET') {
    throw 'Credential environment marker is not BYBIT_TESTNET.'
}

$previousHttpProxy = $env:HTTP_PROXY
$previousHttpsProxy = $env:HTTPS_PROXY
$internetSettings = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
$proxyTarget = if ([string]$internetSettings.ProxyServer -match '(?:^|;)https=([^;]+)') {
    $Matches[1]
}
elseif ([string]$internetSettings.ProxyServer -notmatch '=') {
    [string]$internetSettings.ProxyServer
}
else {
    ''
}
if ($internetSettings.ProxyEnable -eq 1 -and $proxyTarget) {
    $proxyUri = if ($proxyTarget -match '^https?://') {
        [Uri]$proxyTarget
    }
    else {
        [Uri]("http://" + $proxyTarget)
    }
    if ($proxyUri.Host -notin @('127.0.0.1', 'localhost', '::1')) {
        throw 'Refusing to pass API credentials through a non-loopback Windows proxy.'
    }
    $env:HTTP_PROXY = $proxyUri.AbsoluteUri
    $env:HTTPS_PROXY = $proxyUri.AbsoluteUri
}

try {
    $env:IRONPILOT_BYBIT_TESTNET_API_KEY = ConvertFrom-LocalSecureString $credential.ApiKey
    $env:IRONPILOT_BYBIT_TESTNET_API_SECRET = ConvertFrom-LocalSecureString $credential.ApiSecret
    $env:IRONPILOT_BYBIT_TESTNET_WRITE_AUTHORIZATION = 'P4-02A:BYBIT-TESTNET:SPOT:WRITE'
    $env:IRONPILOT_BYBIT_TESTNET_SOCKS5_PROXY = "$($proxyUri.Host):$($proxyUri.Port)"
    cargo run -p ironpilot-adapters --example bybit_testnet_smoke --locked -- $DatabasePath
    if ($LASTEXITCODE -ne 0) {
        throw "P4-02A Testnet smoke exited with code $LASTEXITCODE"
    }
}
finally {
    Remove-Item Env:IRONPILOT_BYBIT_TESTNET_API_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:IRONPILOT_BYBIT_TESTNET_API_SECRET -ErrorAction SilentlyContinue
    Remove-Item Env:IRONPILOT_BYBIT_TESTNET_WRITE_AUTHORIZATION -ErrorAction SilentlyContinue
    Remove-Item Env:IRONPILOT_BYBIT_TESTNET_SOCKS5_PROXY -ErrorAction SilentlyContinue
    if ($null -eq $previousHttpProxy) {
        Remove-Item Env:HTTP_PROXY -ErrorAction SilentlyContinue
    }
    else {
        $env:HTTP_PROXY = $previousHttpProxy
    }
    if ($null -eq $previousHttpsProxy) {
        Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue
    }
    else {
        $env:HTTPS_PROXY = $previousHttpsProxy
    }
}
