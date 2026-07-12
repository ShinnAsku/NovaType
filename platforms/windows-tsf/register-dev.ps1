param(
    [switch]$Unregister,
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Dll = Join-Path $Repo "target\$Configuration\novatype_tsf.dll"

Push-Location $Repo
try {
    cargo build --profile $Configuration -p novatype-tsf
}
finally {
    Pop-Location
}

if (-not (Test-Path $Dll)) {
    throw "TSF DLL was not built: $Dll"
}

$env:NOVATYPE_TSF_DLL_PATH = $Dll
try {
    if ($Unregister) {
        & regsvr32.exe /u /s $Dll
        Write-Host "NovaType TSF unregistered: $Dll"
    }
    else {
        & regsvr32.exe /s $Dll
        Write-Host "NovaType TSF registered: $Dll"
        Write-Host "Note: this skeleton registers COM metadata only; ITfTextInputProcessor implementation is next."
    }
}
finally {
    Remove-Item Env:\NOVATYPE_TSF_DLL_PATH -ErrorAction SilentlyContinue
}
