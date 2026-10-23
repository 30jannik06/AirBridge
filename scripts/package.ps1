# Baut die portable ZIP-Version: dist\AirBridge-<Version>-portable.zip
# Aufruf: .\scripts\package.ps1 -Version 0.1.0 [-SkipBuild]
param(
    [string]$Version = "0.1.0",
    [switch]$SkipBuild
)
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

if (-not $SkipBuild) {
    # Ohne Feature "nonfree-audio": FDK-AAC ist nicht GPL-kompatibel; Musik (ALAC) ist trotzdem dabei.
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build fehlgeschlagen" }
}

$name = "AirBridge-$Version-portable"
$stage = "dist\$name"
if (Test-Path "dist") { Remove-Item -Recurse -Force "dist" }
New-Item -ItemType Directory -Force "$stage\licenses" | Out-Null

Copy-Item "target\release\airbridge.exe" $stage
Set-Content "$stage\portable.txt" -Encoding utf8 "Diese Datei macht AirBridge portabel: Der Schlüssel wird neben der Exe gespeichert.`r`nThis file makes AirBridge portable: the key is stored next to the exe."
Copy-Item "LICENSE", "THIRD_PARTY_NOTICES.md", "README.md", "README.de.md" $stage
Copy-Item "licenses\*" "$stage\licenses"

Compress-Archive -Path "$stage\*" -DestinationPath "dist\$name.zip"
Write-Host "Fertig: dist\$name.zip"
