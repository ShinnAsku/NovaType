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
$Clsid = '{7E4B71B0-5C48-45E8-9E4E-4DFD16FE5E95}'
$KeyboardCategory = '{34745C63-B2F0-4784-8B67-5E12C8701A31}' # GUID_TFCAT_TIP_KEYBOARD
$SortOrderKey = "HKCU:\Software\Microsoft\CTF\SortOrder\AssemblyItem\0x00000804\$KeyboardCategory"
$UserProfileKey = 'HKCU:\Control Panel\International\User Profile\zh-Hans-CN'
$TipValue = "0804:$Clsid$Clsid"

try {
    if ($Unregister) {
        # Remove per-user enablement first.
        if (Test-Path $UserProfileKey) {
            Remove-ItemProperty -Path $UserProfileKey -Name $TipValue -ErrorAction SilentlyContinue
        }
        if (Test-Path $SortOrderKey) {
            Get-ChildItem $SortOrderKey | Where-Object { $_.GetValue('CLSID') -eq $Clsid } |
                Remove-Item -Recurse -Force
        }
        & regsvr32.exe /u /s $Dll
        Write-Host "NovaType TSF unregistered: $Dll"
    }
    else {
        & regsvr32.exe /s $Dll
        Write-Host "NovaType TSF registered: $Dll"

        # Enable the profile for the current user so it appears in the
        # Win+Space input switcher next to Microsoft Pinyin.
        $existing = @()
        if (Test-Path $SortOrderKey) {
            $existing = Get-ChildItem $SortOrderKey | Where-Object { $_.GetValue('CLSID') -eq $Clsid }
        }
        if (-not $existing) {
            $slots = @(Get-ChildItem $SortOrderKey -ErrorAction SilentlyContinue)
            $slot = "{0:D8}" -f $slots.Count
            $entry = New-Item -Path (Join-Path $SortOrderKey $slot) -Force
            Set-ItemProperty $entry.PSPath -Name CLSID -Value $Clsid
            Set-ItemProperty $entry.PSPath -Name KeyboardLayout -Value 0 -Type DWord
            Set-ItemProperty $entry.PSPath -Name Profile -Value $Clsid
        }
        Set-ItemProperty $UserProfileKey -Name $TipValue -Value 1 -Type DWord
        Write-Host "NovaType enabled for current user (Win+Space to switch)."
    }
}
finally {
    Remove-Item Env:\NOVATYPE_TSF_DLL_PATH -ErrorAction SilentlyContinue
}
