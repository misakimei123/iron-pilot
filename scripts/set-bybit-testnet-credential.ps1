[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$credentialDirectory = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.ironpilot'
$credentialPath = Join-Path $credentialDirectory 'bybit-testnet-credential.clixml'

New-Item -ItemType Directory -Path $credentialDirectory -Force | Out-Null

Write-Host 'IronPilot P4-02A - Bybit Testnet credential setup'
Write-Host 'The values are not echoed and are encrypted for the current Windows user with DPAPI.'
$apiKey = Read-Host 'Bybit Testnet API Key' -AsSecureString
$apiSecret = Read-Host 'Bybit Testnet API Secret' -AsSecureString

if ($apiKey.Length -eq 0 -or $apiSecret.Length -eq 0) {
    throw 'API key and secret must both be non-empty.'
}

[pscustomobject]@{
    ApiKey = $apiKey
    ApiSecret = $apiSecret
    Environment = 'BYBIT_TESTNET'
    CreatedAtUtc = [DateTime]::UtcNow.ToString('O')
} | Export-Clixml -LiteralPath $credentialPath -Force

Write-Host "Encrypted credential stored at $credentialPath"
Read-Host 'Press Enter to close'
