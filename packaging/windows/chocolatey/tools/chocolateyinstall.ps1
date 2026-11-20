# Template: packaging/windows/channels.py fills in the @...@ fields from the release's
# SHA256SUMS.txt. Chocolatey shims froxel.exe from the unpacked folder onto the PATH and
# removes both on uninstall.
$ErrorActionPreference = 'Stop'
$toolsDir = Split-Path -Parent $MyInvocation.MyCommand.Definition

$package = @{
  PackageName    = $env:ChocolateyPackageName
  UnzipLocation  = $toolsDir
  Url            = '@URL_X86@'
  Checksum       = '@SHA256_X86@'
  ChecksumType   = 'sha256'
  Url64bit       = '@URL_X64@'
  Checksum64     = '@SHA256_X64@'
  ChecksumType64 = 'sha256'
}
# Chocolatey knows 32 and 64 bit only; an ARM64 Windows gets the native build.
if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') {
  $package.Url64bit = '@URL_ARM64@'
  $package.Checksum64 = '@SHA256_ARM64@'
}
Install-ChocolateyZipPackage @package
