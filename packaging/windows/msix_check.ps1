# Installs an MSIX from msix.ps1 the way the Store would, to check it before upload:
# signs a copy with a throwaway certificate whose subject is the package's Publisher,
# trusts that certificate, installs the copy, runs `froxel --help` through the
# execution alias, checks the .vox association, and removes it all again. Needs an
# elevated shell (GitHub's Windows runners are). The MSIX itself stays unsigned.
#
#   pwsh packaging/windows/msix_check.ps1 -Msix dist/msix/froxel-0.3.3-windows-x64.msix
param([Parameter(Mandatory)] [string] $Msix)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*" -Directory |
    Sort-Object { [version]$_.Name } -Descending |
    Where-Object { Test-Path "$($_.FullName)\x64\signtool.exe" } |
    Select-Object -First 1
if (-not $sdk) { throw 'No Windows SDK with signtool.exe under Windows Kits\10\bin' }
$signtool = "$($sdk.FullName)\x64\signtool.exe"

$work = Join-Path ([System.IO.Path]::GetTempPath()) "froxel-msix-check-$PID"
New-Item -ItemType Directory -Force -Path $work | Out-Null
$copy = Join-Path $work (Split-Path -Leaf $Msix)
Copy-Item $Msix $copy

# The Publisher the certificate must match is in the package's own manifest.
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($copy)
try {
    $reader = [System.IO.StreamReader]::new($zip.GetEntry('AppxManifest.xml').Open())
    [xml] $manifest = $reader.ReadToEnd()
    $reader.Close()
} finally { $zip.Dispose() }
$identity = $manifest.Package.Identity
Write-Output "Package $($identity.Name) $($identity.Version) $($identity.ProcessorArchitecture), publisher $($identity.Publisher)"

$cert = New-SelfSignedCertificate -Type Custom -Subject $identity.Publisher `
    -KeyUsage DigitalSignature -FriendlyName 'Froxel MSIX check' `
    -CertStoreLocation Cert:\CurrentUser\My `
    -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
$trusted = $null
$package = $null
try {
    & $signtool sign /fd SHA256 /sha1 $cert.Thumbprint /s My $copy
    if ($LASTEXITCODE) { throw "signtool sign failed ($LASTEXITCODE)" }
    $cer = Join-Path $work 'check.cer'
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    $trusted = Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople

    Add-AppxPackage -Path $copy
    $package = Get-AppxPackage -Name $identity.Name
    if (-not $package) { throw "$($identity.Name) did not install" }
    Write-Output "Installed $($package.PackageFullName) in $($package.InstallLocation)"

    $alias = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\froxel.exe'
    if (-not (Test-Path $alias)) { throw "No execution alias at $alias" }
    $help = & cmd /c "`"$alias`" --help 2>&1"
    if ($LASTEXITCODE) { throw "froxel --help through the alias exited $LASTEXITCODE" }
    if (-not ($help -match 'usage: froxel')) { throw "froxel --help printed no usage:`n$help" }
    Write-Output "froxel --help through the alias: ok"

    # A packaged association shows as an AppX… ProgID under the extension's OpenWithProgids.
    $ext = Get-Item 'Registry::HKEY_CURRENT_USER\Software\Classes\.vox\OpenWithProgids' -ErrorAction SilentlyContinue
    # @() outside the if: assigning an if's output unrolls an empty or one-item array.
    $progids = @(if ($ext) { $ext.GetValueNames() | Where-Object { $_ -like 'AppX*' } })
    if ($progids.Count -eq 0) { throw 'The .vox file type association is not registered' }
    Write-Output ".vox association: $($progids -join ', ')"
} finally {
    if ($package) { Remove-AppxPackage -Package $package.PackageFullName }
    if ($trusted) { Remove-Item "Cert:\LocalMachine\TrustedPeople\$($trusted.Thumbprint)" }
    Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)"
    Remove-Item -Recurse -Force $work
}
