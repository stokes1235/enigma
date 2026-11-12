param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('gameserver', 'sdkserver', 'muipserver')]
    [string]$Service
)

$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$env:ENIGMA_CONFIG = Join-Path $PSScriptRoot 'config.toml'
$env:RUST_MIN_STACK = '16777216'
$program = Join-Path $PSScriptRoot ($Service + '.exe')
if (-not (Test-Path -LiteralPath $program)) {
    Write-Error ('Missing server executable: ' + $program)
    exit 1
}
$logDirectory = Join-Path $PSScriptRoot 'logs'
New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
$logFile = Join-Path $logDirectory ($Service + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.log')
Write-Host ('Log: ' + $logFile)
# Windows PowerShell treats native stderr as an error record. Keep recording
# it without terminating the logging pipeline while the server is running.
$ErrorActionPreference = 'Continue'
& $program 2>&1 | Tee-Object -FilePath $logFile
$serverExit = $LASTEXITCODE
if ($serverExit -ne 0) {
    Write-Host ('Server exited with code ' + $serverExit + '. See ' + $logFile)
}
exit $serverExit
