# orin installer for Windows.
#
#   irm https://github.com/dnh33/orin/raw/main/install.ps1 | iex
#
# Downloads the Windows archive from the newest GitHub release (it carries
# the one product binary, orin.exe), drops the byte-identical `on.exe` copy, verifies the binary starts, and puts the install
# folder on the user PATH. Running it again replaces the files in place.
# Administrator rights are never required: everything lands under
# %LOCALAPPDATA%\orin.

$ErrorActionPreference = 'Stop'

$homeUrl = 'https://github.com/dnh33/orin'
$ownerRepo = ($homeUrl -replace '^https://github\.com/', '')
# One product binary is downloaded; the short names are local copies of it.
$binaries = @('orin.exe')
$aliases = @('on.exe')
$installDir = Join-Path $env:LOCALAPPDATA 'orin'
$assetPattern = '*-windows-x86_64.zip'

function Write-Step([string]$text) {
    Write-Host "orin  $text"
}

# GitHub rejects requests without a user agent; older PowerShell defaults to TLS 1.0.
$ghHeaders = @{ 'User-Agent' = 'orin-installer'; 'Accept' = 'application/vnd.github+json' }
try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
} catch { }

# 1. Resolve the newest release and its Windows archive.
Write-Step 'checking the latest release'
$release = $null
try {
    $release = Invoke-RestMethod -Headers $ghHeaders -Uri "https://api.github.com/repos/$ownerRepo/releases/latest"
} catch {
    throw "could not read the latest release from GitHub: $($_.Exception.Message)"
}

$asset = @($release.assets) | Where-Object { $_.name -like $assetPattern } | Select-Object -First 1
if (-not $asset) {
    throw "release $($release.tag_name) has no asset matching $assetPattern"
}
Write-Step "found $($asset.name) in $($release.tag_name)"

# 2. Replace files on disk: stop the running daemon (the orin process) first
#    so nothing is locked.
$running = @(Get-Process -Name 'orin' -ErrorAction SilentlyContinue)
if ($running.Count -gt 0) {
    Write-Step 'stopping the running daemon so its files can be replaced'
    $running | Stop-Process -Force
    Start-Sleep -Milliseconds 500
}

# 3. Download and extract.
$stage = Join-Path $env:TEMP ("orin-install-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage -Force | Out-Null
$zip = Join-Path $stage $asset.name

try {
    Write-Step "downloading $($asset.browser_download_url)"
    Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url -OutFile $zip
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Expand-Archive -Path $zip -DestinationPath $stage -Force

    # The archive may nest the binaries in a folder; find them either way.
    foreach ($name in $binaries) {
        $found = Get-ChildItem -Path $stage -Recurse -Filter $name | Select-Object -First 1
        if (-not $found) {
            throw "$name is missing from $($asset.name)"
        }
        Copy-Item -Path $found.FullName -Destination (Join-Path $installDir $name) -Force
    }

    # Short names are byte-identical copies of orin.exe, never separate
    # downloads: the argv[0] stem `on` makes the copy run `orin query`.
    # Copy-Item -Force makes this idempotent on re-runs.
    $product = Join-Path $installDir 'orin.exe'
    foreach ($name in $aliases) {
        Copy-Item -Path $product -Destination (Join-Path $installDir $name) -Force
        Write-Step "  $name is a byte-identical copy of orin.exe"
    }
} finally {
    Remove-Item -Path $stage -Recurse -Force -ErrorAction SilentlyContinue
}

# 4. Verify the product binary actually starts.
Write-Step 'verifying the binary'
foreach ($name in $binaries) {
    $exe = Join-Path $installDir $name
    $output = (& $exe --version)
    if ($LASTEXITCODE -ne 0) {
        throw "$name --version exited with code $LASTEXITCODE"
    }
    Write-Step ("  {0,-11} {1}" -f $name, (@($output) -join ' ').Trim())
}

# 5. Add the install folder to the user PATH (once, and without touching the system PATH).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($null -eq $userPath) { $userPath = '' }
$entries = @($userPath -split ';' | Where-Object { $_ -ne '' })
$known = $entries | Where-Object { $_.TrimEnd('\') -ieq $installDir.TrimEnd('\') }
if (-not $known) {
    $newPath = ($entries + $installDir) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    $env:Path = "$env:Path;$installDir"
    Write-Step "added $installDir to the user PATH"
} else {
    Write-Step 'the user PATH already contains the install folder'
}

Write-Step "installed to $installDir"
Write-Step 'open a new terminal, then run:  orin status'
Write-Step 'short alias:                  on <terms> (== orin query <terms>)'
