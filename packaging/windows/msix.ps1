# Packs one Froxel build into an unsigned MSIX for the Microsoft Store, with the
# Windows SDK's makepri and makeappx. The Store signs what it publishes.
#
#   pwsh packaging/windows/msix.ps1 -Exe target/x86_64-pc-windows-msvc/release/froxel.exe `
#     -Arch x64 -Build 30399 -Out dist/msix/froxel-0.3.3-windows-x64.msix
#
# -Build is packaging/build_number.sh's number (major * 1000000 + minor * 10000 +
# patch * 100 + stage). MSIX versions are four 16-bit parts and the Store wants the
# last one 0, so the package version is major.minor.(patch * 100 + stage).0:
# 0.3.3 is 0.3.399.0 and 0.3.4-rc.1 is 0.3.401.0, which keeps every upload newer.
# The identity and publisher are Partner Center's (Product identity); the defaults
# only make a package that installs locally after test signing.
param(
    [Parameter(Mandatory)] [string] $Exe,
    [Parameter(Mandatory)] [ValidateSet('x64', 'x86', 'arm64')] [string] $Arch,
    [Parameter(Mandatory)] [int] $Build,
    [Parameter(Mandatory)] [string] $Out,
    [string] $IdentityName = 'Lonje.Froxel',
    [string] $Publisher = 'CN=Froxel Development',
    [string] $PublisherDisplayName = 'Roman Vasilyev'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$root = (Resolve-Path "$PSScriptRoot/../..").Path
$major = [math]::Floor($Build / 1000000)
$minor = [math]::Floor($Build / 10000) % 100
$third = $Build % 10000
$version = "$major.$minor.$third.0"

# The newest SDK's x64 tools run on every runner, including arm64 ones (emulated).
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*" -Directory |
    Sort-Object { [version]$_.Name } -Descending |
    Where-Object { Test-Path "$($_.FullName)\x64\makeappx.exe" } |
    Select-Object -First 1
if (-not $sdk) { throw 'No Windows SDK with makeappx.exe under Windows Kits\10\bin' }
$makeappx = "$($sdk.FullName)\x64\makeappx.exe"
$makepri = "$($sdk.FullName)\x64\makepri.exe"

$stage = Join-Path ([System.IO.Path]::GetTempPath()) "froxel-msix-$Arch-$PID"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$stage/Assets" | Out-Null
Copy-Item $Exe "$stage/froxel.exe"
Copy-Item "$root/README.md", "$root/LICENSE" $stage
Copy-Item "$PSScriptRoot/Assets/*.png" "$stage/Assets"

$manifest = (Get-Content -Raw "$PSScriptRoot/AppxManifest.xml").
    Replace('@IDENTITY_NAME@', [System.Security.SecurityElement]::Escape($IdentityName)).
    Replace('@PUBLISHER@', [System.Security.SecurityElement]::Escape($Publisher)).
    Replace('@PUBLISHER_DISPLAY_NAME@', [System.Security.SecurityElement]::Escape($PublisherDisplayName)).
    Replace('@VERSION@', $version).
    Replace('@ARCH@', $Arch)
if ($manifest -match '@[A-Z_]+@') { throw "AppxManifest.xml still has $($Matches[0])" }
Set-Content -Path "$stage/AppxManifest.xml" -Value $manifest -Encoding utf8NoBOM

# resources.pri maps Assets\Square150x150Logo.png to its scale and target-size files.
$pri = Join-Path ([System.IO.Path]::GetTempPath()) "froxel-priconfig-$Arch-$PID.xml"
& $makepri createconfig /cf $pri /dq en-US /pv 10.0.0 /o
if ($LASTEXITCODE) { throw "makepri createconfig failed ($LASTEXITCODE)" }
& $makepri new /pr $stage /cf $pri /mn "$stage/AppxManifest.xml" /of "$stage/resources.pri" /o
if ($LASTEXITCODE) { throw "makepri new failed ($LASTEXITCODE)" }
Remove-Item $pri

New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null
& $makeappx pack /d $stage /p $Out /o
if ($LASTEXITCODE) { throw "makeappx pack failed ($LASTEXITCODE)" }
Remove-Item -Recurse -Force $stage
Write-Output "$Out ($IdentityName $version $Arch)"
