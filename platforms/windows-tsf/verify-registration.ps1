param(
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Dll = Join-Path $Repo "target\$Configuration\novatype_tsf.dll"
$MarkerPath = "HKCU:\Software\NovaType\TSF"

Push-Location $Repo
try {
    cargo build --profile $Configuration -p novatype-tsf | Out-Host
}
finally {
    Pop-Location
}

if (-not (Test-Path $Dll)) {
    throw "TSF DLL was not built: $Dll"
}

$env:NOVATYPE_TSF_DLL_PATH = $Dll
try {
    & regsvr32.exe /s $Dll
    if ($LASTEXITCODE -ne 0) {
        throw "regsvr32 registration failed with exit code $LASTEXITCODE"
    }

    $marker = Get-ItemProperty -Path $MarkerPath -ErrorAction Stop
    if ($marker.DisplayName -ne "NovaType") {
        throw "Unexpected DisplayName in $MarkerPath"
    }
    if ($marker.ModulePath -ne $Dll) {
        throw "Unexpected ModulePath in ${MarkerPath}: $($marker.ModulePath)"
    }

    Write-Host "TSF registration marker OK: $MarkerPath"
}
finally {
    & regsvr32.exe /u /s $Dll | Out-Null
    Remove-Item Env:\NOVATYPE_TSF_DLL_PATH -ErrorAction SilentlyContinue
}
