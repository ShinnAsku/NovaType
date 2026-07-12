$ErrorActionPreference = "Stop"

$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Dll = Join-Path $Repo "target\release\novatype_tsf.dll"
if (-not (Test-Path $Dll)) {
    Push-Location $Repo
    try {
        cargo build -p novatype-tsf --release
    }
    finally {
        Pop-Location
    }
}

$Dumpbin = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
$required = @("DllRegisterServer", "DllUnregisterServer", "DllCanUnloadNow", "DllGetClassObject")
$exports = if ($Dumpbin) {
    & $Dumpbin.Source /exports $Dll
}
else {
    $bytes = [System.IO.File]::ReadAllBytes($Dll)
    [System.Text.Encoding]::ASCII.GetString($bytes)
}

$missing = @($required | Where-Object { $exports -notmatch $_ })
if ($missing.Count -gt 0) {
    throw "Missing exports: $($missing -join ', ')"
}

Write-Host "TSF exports OK: $($required -join ', ')"
