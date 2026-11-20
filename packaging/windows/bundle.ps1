# Bundles the per-architecture MSIX files from msix.ps1 into one .msixbundle and
# wraps it in the .msixupload Partner Center and `msstore publish` take (a zip of
# the bundle). Neither is signed; the Store signs what it publishes.
#
#   pwsh packaging/windows/bundle.ps1 -Dir dist/msix -Build 30399 -Out dist/store/froxel-0.3.3-windows
#
# writes froxel-0.3.3-windows.msixbundle and froxel-0.3.3-windows.msixupload.
param(
    [Parameter(Mandatory)] [string] $Dir,
    [Parameter(Mandatory)] [int] $Build,
    [Parameter(Mandatory)] [string] $Out,
    # A bundle missing one of these would drop that architecture from the Store.
    [string[]] $Arch = @('x64', 'x86', 'arm64')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$version = "$([math]::Floor($Build / 1000000)).$([math]::Floor($Build / 10000) % 100).$($Build % 10000).0"
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*" -Directory |
    Sort-Object { [version]$_.Name } -Descending |
    Where-Object { Test-Path "$($_.FullName)\x64\makeappx.exe" } |
    Select-Object -First 1
if (-not $sdk) { throw 'No Windows SDK with makeappx.exe under Windows Kits\10\bin' }
$makeappx = "$($sdk.FullName)\x64\makeappx.exe"

$msix = @(Get-ChildItem "$Dir/*.msix")
foreach ($a in $Arch) {
    if (-not ($msix | Where-Object { $_.Name -like "*-windows-$a.msix" })) { throw "No $a .msix in $Dir" }
}
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Out) | Out-Null
& $makeappx bundle /d $Dir /p "$Out.msixbundle" /bv $version /o
if ($LASTEXITCODE) { throw "makeappx bundle failed ($LASTEXITCODE)" }

Remove-Item "$Out.msixupload", "$Out.zip" -ErrorAction SilentlyContinue
Compress-Archive -Path "$Out.msixbundle" -DestinationPath "$Out.zip"
Move-Item "$Out.zip" "$Out.msixupload"
Write-Output "$Out.msixbundle and .msixupload: $version, $($msix.Name -join ', ')"
