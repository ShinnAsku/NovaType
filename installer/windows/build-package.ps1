param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
Push-Location $Repo
try {
    cargo build --profile $Configuration -p novatype-server -p novatype-desktop -p novatype-tsf
    Push-Location apps\desktop
    try {
        npm ci
        npm run build
    }
    finally {
        Pop-Location
    }

    Write-Host "Build artifacts are ready. Run Inno Setup on installer/windows/novatype.iss to create the installer."
}
finally {
    Pop-Location
}
