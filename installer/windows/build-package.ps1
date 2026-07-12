param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Push-Location $Repo
try {
    Write-Host "=== Building Rust targets ($Configuration) ==="
    cargo build --profile $Configuration -p novatype-server -p novatype-desktop -p novatype-tsf
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    Write-Host "`n=== Verifying TSF exports ==="
    & (Join-Path $PSScriptRoot "..\..\platforms\windows-tsf\check-exports.ps1")
    if ($LASTEXITCODE -ne 0) { throw "COM exports check failed" }

    Write-Host "`n=== Checking package size ==="
    & (Join-Path $PSScriptRoot "check-size.ps1") -Configuration $Configuration
    if ($LASTEXITCODE -ne 0) { throw "size check failed" }

    Write-Host "`n=== Building desktop frontend ==="
    Push-Location (Join-Path $Repo "apps\desktop")
    try {
        npm ci
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "npm run build failed" }
    }
    finally {
        Pop-Location
    }

    Write-Host "`n=== Build complete ==="
    Write-Host "Artifacts:"
    Get-Item "$Repo\target\$Configuration\novatype-server.exe" | ForEach-Object { "  $($_.FullName) ($('{0:N2} MB' -f ($_.Length / 1MB)))" }
    Get-Item "$Repo\target\$Configuration\novatype-desktop.exe" | ForEach-Object { "  $($_.FullName) ($('{0:N2} MB' -f ($_.Length / 1MB)))" }
    Get-Item "$Repo\target\$Configuration\novatype_tsf.dll" | ForEach-Object { "  $($_.FullName) ($('{0:N2} MB' -f ($_.Length / 1MB)))" }
    Write-Host ""
    Write-Host "Next: Run Inno Setup Compiler on installer/windows/novatype.iss to produce the .exe installer."
    Write-Host "  iscc installer\windows\novatype.iss"
}
finally {
    Pop-Location
}
