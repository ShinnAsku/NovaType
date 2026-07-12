param(
    [int]$LimitMb = 35,
    [string]$Configuration = "release"
)

$ErrorActionPreference = "Stop"
$Repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$Artifacts = @(
    Join-Path $Repo "target\$Configuration\novatype-server.exe"
    Join-Path $Repo "target\$Configuration\novatype-desktop.exe"
    Join-Path $Repo "target\$Configuration\novatype_tsf.dll"
)

$missing = @($Artifacts | Where-Object { -not (Test-Path $_) })
if ($missing.Count -gt 0) {
    Push-Location $Repo
    try {
        cargo build --profile $Configuration -p novatype-server -p novatype-desktop -p novatype-tsf | Out-Host
    }
    finally {
        Pop-Location
    }
}

$total = 0L
foreach ($artifact in $Artifacts) {
    if (Test-Path $artifact) {
        $item = Get-Item $artifact
        $total += $item.Length
        Write-Host ("{0,8:N2} MB  {1}" -f ($item.Length / 1MB), $item.Name)
    }
}

$limit = $LimitMb * 1MB
Write-Host ("Total: {0:N2} MB / limit {1} MB" -f ($total / 1MB), $LimitMb)
if ($total -gt $limit) {
    throw "Artifact size budget exceeded"
}
