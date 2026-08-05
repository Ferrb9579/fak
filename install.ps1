# One-command installer for Windows PowerShell:
#   irm https://raw.githubusercontent.com/Ferrb9579/fak/master/install.ps1 | iex

$ErrorActionPreference = "Stop"

function Write-Info([string] $Message) {
    Write-Host $Message
}

function Download-File([string] $Url, [string] $Destination) {
    Invoke-WebRequest -Uri $Url -OutFile $Destination -UseBasicParsing -Headers @{ "User-Agent" = "fak-installer" }
}

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repository = "Ferrb9579/fak"
$release = [Environment]::GetEnvironmentVariable("FAK_VERSION")
if ([string]::IsNullOrWhiteSpace($release)) {
    $release = "latest"
} elseif (-not $release.StartsWith("v")) {
    $release = "v$release"
}

$processorArchitecture = [Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITEW6432")
if ([string]::IsNullOrWhiteSpace($processorArchitecture)) {
    $processorArchitecture = [Environment]::GetEnvironmentVariable("PROCESSOR_ARCHITECTURE")
}

switch ($processorArchitecture.ToUpperInvariant()) {
    "AMD64" { $architecture = "x86_64"; break }
    "ARM64" { $architecture = "aarch64"; break }
    default { throw "Unsupported Windows CPU architecture: $processorArchitecture" }
}

$asset = "fak-windows-$architecture.exe"
if ($release -eq "latest") {
    $downloadBase = "https://github.com/$repository/releases/latest/download"
} else {
    $downloadBase = "https://github.com/$repository/releases/download/$release"
}

$temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("fak-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $temporaryDirectory -Force | Out-Null

try {
    $binaryDownload = Join-Path $temporaryDirectory $asset
    $checksumDownload = Join-Path $temporaryDirectory "$asset.sha256"

    Write-Info "Downloading fak for Windows/$architecture..."
    Download-File "$downloadBase/$asset" $binaryDownload
    Download-File "$downloadBase/$asset.sha256" $checksumDownload

    $expectedChecksum = ((Get-Content -LiteralPath $checksumDownload -Raw).Trim() -split "\s+")[0].ToLowerInvariant()
    if ($expectedChecksum -notmatch "^[0-9a-f]{64}$") {
        throw "The release checksum is invalid"
    }

    $actualChecksum = (Get-FileHash -LiteralPath $binaryDownload -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($expectedChecksum -ne $actualChecksum) {
        throw "Checksum verification failed for $asset"
    }

    $configuredInstallDirectory = [Environment]::GetEnvironmentVariable("FAK_INSTALL_DIR")
    if ([string]::IsNullOrWhiteSpace($configuredInstallDirectory)) {
        $localApplicationData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
        $configuredInstallDirectory = Join-Path $localApplicationData "Programs\fak"
    }

    New-Item -ItemType Directory -Path $configuredInstallDirectory -Force | Out-Null
    $binaryPath = Join-Path $configuredInstallDirectory "fak.exe"
    Copy-Item -LiteralPath $binaryDownload -Destination $binaryPath -Force

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @()
    if (-not [string]::IsNullOrWhiteSpace($userPath)) {
        $pathEntries = @($userPath -split ";" | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    }
    $pathAlreadyContainsInstallDirectory = @($pathEntries | Where-Object {
        $_.TrimEnd("\") -ieq $configuredInstallDirectory.TrimEnd("\")
    }).Count -gt 0
    if (-not $pathAlreadyContainsInstallDirectory) {
        $pathEntries += $configuredInstallDirectory
        [Environment]::SetEnvironmentVariable("Path", ($pathEntries -join ";"), "User")
    }

    if (@($env:Path -split ";" | Where-Object {
        $_.TrimEnd("\") -ieq $configuredInstallDirectory.TrimEnd("\")
    }).Count -eq 0) {
        $env:Path = "$configuredInstallDirectory;$env:Path"
    }

    $profilePath = $PROFILE
    $profileDirectory = Split-Path -Parent $profilePath
    if (-not [string]::IsNullOrWhiteSpace($profileDirectory)) {
        New-Item -ItemType Directory -Path $profileDirectory -Force | Out-Null
    }
    if (-not (Test-Path -LiteralPath $profilePath)) {
        New-Item -ItemType File -Path $profilePath -Force | Out-Null
    }

    $escapedBinaryPath = $binaryPath.Replace("'", "''")
    $hook = @'
# >>> fak shell integration >>>
function fak {
    $oldHistory = $env:FAK_HISTORY
    $oldShell = $env:FAK_SHELL
    $oldAlias = $env:FAK_ALIAS
    try {
        $env:FAK_SHELL = 'powershell'
        $env:FAK_ALIAS = 'fak'
        $historyLines = @(Get-History -Count 10 | ForEach-Object { $_.CommandLine })
        $env:FAK_HISTORY = ($historyLines -join [Environment]::NewLine)
        $fixedOutput = & '__FAK_BINARY__' --shell-command @args
        $status = $LASTEXITCODE
        $fixed = ($fixedOutput -join [Environment]::NewLine).Trim()
        if ($status -ne 0 -or [string]::IsNullOrWhiteSpace($fixed)) {
            return
        }
        try {
            [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory($fixed)
        } catch {
            # PSReadLine is optional; command execution still works without it.
        }
        Invoke-Expression $fixed
    } finally {
        if ($null -eq $oldHistory) { Remove-Item Env:FAK_HISTORY -ErrorAction SilentlyContinue } else { $env:FAK_HISTORY = $oldHistory }
        if ($null -eq $oldShell) { Remove-Item Env:FAK_SHELL -ErrorAction SilentlyContinue } else { $env:FAK_SHELL = $oldShell }
        if ($null -eq $oldAlias) { Remove-Item Env:FAK_ALIAS -ErrorAction SilentlyContinue } else { $env:FAK_ALIAS = $oldAlias }
    }
}
# <<< fak shell integration <<<
'@
    $hook = $hook.Replace("__FAK_BINARY__", $escapedBinaryPath)

    $profileText = Get-Content -LiteralPath $profilePath -Raw
    if ($profileText -notmatch "# >>> fak shell integration >>>") {
        Add-Content -LiteralPath $profilePath -Value ([Environment]::NewLine + $hook)
    }

    # Make fak available in the PowerShell session that ran the installer too.
    . $profilePath

    & $binaryPath --help | Out-Null
    Write-Info "fak was installed at $binaryPath"
    Write-Info "The user PATH and PowerShell profile were updated."
    Write-Info "Try now: git statuss, then fak"
    Write-Info "Git Bash users can run install.sh instead for Bash history integration."
} finally {
    Remove-Item -LiteralPath $temporaryDirectory -Recurse -Force -ErrorAction SilentlyContinue
}
