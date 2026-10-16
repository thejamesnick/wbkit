# WBKit installer for Windows: downloads the right program, checks its SHA-256, installs it and adds it to your PATH.
# Options: $env:WBKIT_VERSION = 'v0.1.0' (default: latest), $env:WBKIT_INSTALL_DIR (default: %LOCALAPPDATA%\wbkit\bin)
$ErrorActionPreference = 'Stop'
$repo = 'thejamesnick/wbkit'
$arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64' -or $env:PROCESSOR_ARCHITEW6432 -eq 'ARM64') { 'arm64' } else { 'x64' }
$asset = "wbkit-win32-$arch.exe"
$version = if ($env:WBKIT_VERSION) { $env:WBKIT_VERSION } else { 'latest' }
$base = if ($version -eq 'latest') { "https://github.com/$repo/releases/latest/download" } else { "https://github.com/$repo/releases/download/$version" }
$dir = if ($env:WBKIT_INSTALL_DIR) { $env:WBKIT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'wbkit\bin' }
$tmp = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
try {
  Write-Host "Downloading $asset ($version)..."
  Invoke-WebRequest -UseBasicParsing "$base/$asset" -OutFile (Join-Path $tmp $asset)
  Invoke-WebRequest -UseBasicParsing "$base/SHA256SUMS" -OutFile (Join-Path $tmp 'SHA256SUMS')
  $line = Get-Content (Join-Path $tmp 'SHA256SUMS') | Where-Object { $_ -match " $([regex]::Escape($asset))$" }
  if (-not $line) { throw "No checksum listed for $asset" }
  $expected = ($line -split ' ')[0]
  $actual = (Get-FileHash (Join-Path $tmp $asset) -Algorithm SHA256).Hash.ToLower()
  if ($expected -ne $actual) { throw 'Checksum mismatch; the download may be damaged' }
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  Move-Item -Force (Join-Path $tmp $asset) (Join-Path $dir 'wbkit.exe')
  # Downloaded files are marked as coming from the internet; this one has just been checked.
  Unblock-File (Join-Path $dir 'wbkit.exe')
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (($userPath -split ';') -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', ($(if ($userPath) { "$userPath;" } else { '' }) + $dir), 'User')
    Write-Host "Added $dir to your PATH. Open a new terminal to use wbkit."
  }
  & (Join-Path $dir 'wbkit.exe') --version
} finally { Remove-Item -Recurse -Force $tmp }
