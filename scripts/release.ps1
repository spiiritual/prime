<#
.SYNOPSIS
Bump Prime's Cargo version, package with Velopack, and optionally publish a GitHub release.

.EXAMPLE
.\scripts\release.ps1 -UseGeneratedNotes -Publish

Bumps the patch version, runs cargo check/test/build, packages the win channel, commits,
tags, pushes, and creates the GitHub release using generated git-log notes.

.EXAMPLE
.\scripts\release.ps1 -Version 0.2.0 -ReleaseNotesFile .\notes.md -Publish

Releases an explicit version with curated release notes.
#>
[CmdletBinding()]
param(
    [ValidateSet("patch", "minor", "major")]
    [string] $Bump = "patch",

    [ValidatePattern('^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)([-+][0-9A-Za-z.-]+)?$')]
    [string] $Version,

    [ValidatePattern('^[A-Za-z0-9._-]+$')]
    [string] $Channel = "win",

    [Alias("Notes")]
    [string] $ReleaseNotes,

    [Alias("NotesFile")]
    [string] $ReleaseNotesFile,

    [switch] $UseGeneratedNotes,
    [switch] $Publish,
    [switch] $Draft,
    [switch] $Prerelease,
    [switch] $AllowDirty,
    [switch] $SkipTests
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Command,

        [string[]] $Arguments = @()
    )

    Write-Host ">> $Command $($Arguments -join ' ')"
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Command $($Arguments -join ' ')"
    }
}

function Invoke-Captured {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Command,

        [string[]] $Arguments = @(),
        [switch] $AllowFailure
    )

    $output = & $Command @Arguments 2>&1
    $exitCode = $LASTEXITCODE
    if (($exitCode -ne 0) -and (-not $AllowFailure)) {
        $text = ($output | Out-String).Trim()
        throw "Command failed with exit code ${exitCode}: $Command $($Arguments -join ' ')`n$text"
    }

    return @{
        ExitCode = $exitCode
        Output = (($output | Out-String).Trim())
    }
}

function Require-Command {
    param([Parameter(Mandatory = $true)] [string] $Name)

    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) {
        throw "Required command '$Name' was not found on PATH."
    }

    return $command.Source
}

function Write-Utf8File {
    param(
        [Parameter(Mandatory = $true)] [string] $Path,
        [Parameter(Mandatory = $true)] [string] $Value
    )

    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($Path, $Value, $utf8NoBom)
}

function Get-PackageVersion {
    param([Parameter(Mandatory = $true)] [string] $CargoTomlPath)

    $text = Get-Content -LiteralPath $CargoTomlPath -Raw
    $packageMatch = [regex]::Match($text, '(?ms)^\[package\]\r?\n(?<body>.*?)(?=^\[|\z)')
    if (-not $packageMatch.Success) {
        throw "Could not find [package] in Cargo.toml."
    }

    $versionMatch = [regex]::Match($packageMatch.Value, '(?m)^(?<prefix>version\s*=\s*")(?<version>[^"]+)(?<suffix>")')
    if (-not $versionMatch.Success) {
        throw "Could not find package.version in Cargo.toml."
    }

    return $versionMatch.Groups["version"].Value
}

function Set-PackageVersion {
    param(
        [Parameter(Mandatory = $true)] [string] $CargoTomlPath,
        [Parameter(Mandatory = $true)] [string] $NewVersion
    )

    $text = Get-Content -LiteralPath $CargoTomlPath -Raw
    $packageMatch = [regex]::Match($text, '(?ms)^\[package\]\r?\n(?<body>.*?)(?=^\[|\z)')
    if (-not $packageMatch.Success) {
        throw "Could not find [package] in Cargo.toml."
    }

    $versionRegex = [regex]::new('(?m)^(?<prefix>version\s*=\s*")(?<version>[^"]+)(?<suffix>")')
    $updatedPackage = $versionRegex.Replace($packageMatch.Value, "`${prefix}$NewVersion`${suffix}", 1)

    if ($updatedPackage -eq $packageMatch.Value) {
        throw "Could not update package.version in Cargo.toml."
    }

    $updatedText = $text.Remove($packageMatch.Index, $packageMatch.Length).Insert($packageMatch.Index, $updatedPackage)
    Write-Utf8File $CargoTomlPath $updatedText
}

function Get-LockPackageVersion {
    param(
        [Parameter(Mandatory = $true)] [string] $CargoLockPath,
        [Parameter(Mandatory = $true)] [string] $PackageName
    )

    if (-not (Test-Path -LiteralPath $CargoLockPath)) {
        return $null
    }

    $text = Get-Content -LiteralPath $CargoLockPath -Raw
    $packageMatches = [regex]::Matches($text, '(?ms)^\[\[package\]\]\r?\n(?<body>.*?)(?=^\[\[package\]\]|\z)')
    foreach ($packageMatch in $packageMatches) {
        $block = $packageMatch.Value
        $nameMatch = [regex]::Match($block, '(?m)^name\s*=\s*"(?<name>[^"]+)"')
        if (-not $nameMatch.Success -or $nameMatch.Groups["name"].Value -ne $PackageName) {
            continue
        }

        $versionMatch = [regex]::Match($block, '(?m)^version\s*=\s*"(?<version>[^"]+)"')
        if (-not $versionMatch.Success) {
            throw "Found package '$PackageName' in Cargo.lock but it has no version."
        }

        return $versionMatch.Groups["version"].Value
    }

    return $null
}

function ConvertTo-NextVersion {
    param(
        [Parameter(Mandatory = $true)] [string] $CurrentVersion,
        [Parameter(Mandatory = $true)] [string] $BumpKind
    )

    $match = [regex]::Match($CurrentVersion, '^(?<major>0|[1-9]\d*)\.(?<minor>0|[1-9]\d*)\.(?<patch>0|[1-9]\d*)$')
    if (-not $match.Success) {
        throw "Automatic bumping only supports stable x.y.z versions. Current version is '$CurrentVersion'. Pass -Version explicitly."
    }

    $major = [int] $match.Groups["major"].Value
    $minor = [int] $match.Groups["minor"].Value
    $patch = [int] $match.Groups["patch"].Value

    switch ($BumpKind) {
        "major" {
            $major += 1
            $minor = 0
            $patch = 0
        }
        "minor" {
            $minor += 1
            $patch = 0
        }
        "patch" {
            $patch += 1
        }
    }

    return "$major.$minor.$patch"
}

function Get-LastReleaseTag {
    $result = Invoke-Captured git @("describe", "--tags", "--match", "v[0-9]*", "--abbrev=0") -AllowFailure
    if ($result.ExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($result.Output)) {
        return $null
    }

    return $result.Output.Trim()
}

function New-GeneratedReleaseNotes {
    param(
        [Parameter(Mandatory = $true)] [string] $NewVersion
    )

    $lastTag = Get-LastReleaseTag
    if ($lastTag) {
        $range = "$lastTag..HEAD"
        $log = (Invoke-Captured git @("log", "--pretty=format:- %s (%h)", $range)).Output
    } else {
        $log = (Invoke-Captured git @("log", "--pretty=format:- %s (%h)")).Output
    }

    if ([string]::IsNullOrWhiteSpace($log)) {
        $log = "- Release packaging updates."
    }

    return "# Prime $NewVersion`n`n## Changes`n$log`n"
}

function Resolve-Velopack {
    $vpk = Get-Command vpk -ErrorAction SilentlyContinue
    if ($vpk) {
        return $vpk.Source
    }

    $toolPath = Join-Path $env:USERPROFILE ".dotnet\tools\vpk.exe"
    if (Test-Path -LiteralPath $toolPath) {
        return $toolPath
    }

    throw "Velopack CLI was not found. Install it or make vpk available on PATH."
}

function Assert-LocalTagMissing {
    param([Parameter(Mandatory = $true)] [string] $Tag)

    $result = Invoke-Captured git @("rev-parse", "-q", "--verify", "refs/tags/$Tag") -AllowFailure
    if ($result.ExitCode -eq 0) {
        throw "Local tag '$Tag' already exists."
    }
}

function Assert-RemoteTagMissing {
    param(
        [Parameter(Mandatory = $true)] [string] $Remote,
        [Parameter(Mandatory = $true)] [string] $Tag
    )

    $result = Invoke-Captured git @("ls-remote", "--tags", $Remote, "refs/tags/$Tag") -AllowFailure
    if (($result.ExitCode -eq 0) -and (-not [string]::IsNullOrWhiteSpace($result.Output))) {
        throw "Remote tag '$Tag' already exists on '$Remote'."
    }
}

if ($ReleaseNotes -and $ReleaseNotesFile) {
    throw "Use either -ReleaseNotes or -ReleaseNotesFile, not both."
}

if ($Publish -and (-not $ReleaseNotes) -and (-not $ReleaseNotesFile) -and (-not $UseGeneratedNotes)) {
    throw "Publishing with generated notes requires -UseGeneratedNotes. Or pass -ReleaseNotes/-ReleaseNotesFile."
}

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = (Resolve-Path (Join-Path $scriptDir "..")).Path
$cargoToml = Join-Path $root "Cargo.toml"
$cargoLock = Join-Path $root "Cargo.lock"
$targetDir = Join-Path $root "target"
$stageDir = Join-Path $targetDir "velopack-stage"
$releaseDir = Join-Path $targetDir "velopack-releases"
$remote = "origin"

Set-Location $root

Require-Command git | Out-Null
Require-Command cargo | Out-Null
if ($Publish) {
    Require-Command gh | Out-Null
}

$status = (Invoke-Captured git @("status", "--porcelain")).Output
if ((-not [string]::IsNullOrWhiteSpace($status)) -and (-not $AllowDirty)) {
    throw "The worktree has uncommitted changes. Commit or stash them first, or rerun with -AllowDirty to stage only Cargo.toml/Cargo.lock."
}

$currentVersion = Get-PackageVersion $cargoToml
if (-not $Version) {
    $Version = ConvertTo-NextVersion $currentVersion $Bump
}

if ($Version -eq $currentVersion) {
    throw "Target version '$Version' is already the package version."
}

$tag = "v$Version"
Assert-LocalTagMissing $tag
if ($Publish) {
    Assert-RemoteTagMissing $remote $tag
    Invoke-Checked gh @("auth", "status")
    $existingRelease = Invoke-Captured gh @("release", "view", $tag) -AllowFailure
    if ($existingRelease.ExitCode -eq 0) {
        throw "GitHub release '$tag' already exists."
    }
}

Write-Host "Releasing Prime $currentVersion -> $Version on channel '$Channel'."

if (-not (Test-Path -LiteralPath $targetDir)) {
    New-Item -ItemType Directory -Path $targetDir | Out-Null
}

$notesPath = Join-Path $targetDir "release-notes-$Version.md"
if ($ReleaseNotesFile) {
    $resolvedNotes = (Resolve-Path -LiteralPath $ReleaseNotesFile).Path
    Copy-Item -LiteralPath $resolvedNotes -Destination $notesPath -Force
} elseif ($ReleaseNotes) {
    Write-Utf8File $notesPath $ReleaseNotes
} else {
    Write-Utf8File $notesPath (New-GeneratedReleaseNotes $Version)
}

Set-PackageVersion $cargoToml $Version

Invoke-Checked cargo @("check")
$lockVersion = Get-LockPackageVersion $cargoLock "prime"
if ($lockVersion -ne $Version) {
    Invoke-Checked cargo @("update", "-p", "prime")
    Invoke-Checked cargo @("check")
    $lockVersion = Get-LockPackageVersion $cargoLock "prime"
    if ($lockVersion -ne $Version) {
        throw "Cargo.lock still has prime version '$lockVersion' after updating; expected '$Version'."
    }
}

if (-not $SkipTests) {
    Invoke-Checked cargo @("test")
} else {
    Write-Warning "Skipping cargo test because -SkipTests was passed."
}
Invoke-Checked cargo @("build", "--release")

if (Test-Path -LiteralPath $stageDir) {
    Remove-Item -LiteralPath $stageDir -Recurse -Force
}
New-Item -ItemType Directory -Path $stageDir | Out-Null
New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null

$releaseExe = Join-Path $root "target\release\prime.exe"
if (-not (Test-Path -LiteralPath $releaseExe)) {
    throw "Expected release executable was not found: $releaseExe"
}
Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $stageDir "prime.exe")

$vpk = Resolve-Velopack
Invoke-Checked $vpk @(
    "--yes",
    "--skip-updates",
    "pack",
    "--packId", "dev.spiiritual.prime",
    "--packVersion", $Version,
    "--packDir", $stageDir,
    "--mainExe", "prime.exe",
    "--packTitle", "Prime",
    "--icon", (Join-Path $root "assets\icon.ico"),
    "--packAuthors", "spiiritual",
    "--channel", $Channel,
    "--runtime", "win-x64",
    "--outputDir", $releaseDir,
    "--releaseNotes", $notesPath
)

# Velopack leaves the channel out of package and RELEASES names for the default "win" channel
# (dev.spiiritual.prime-1.2.3-full.nupkg, RELEASES) but includes it for others (-beta-full.nupkg,
# RELEASES-beta).
$channelSuffix = if ($Channel -eq "win") { "" } else { "-$Channel" }

$requiredArtifacts = @(
    (Join-Path $releaseDir "dev.spiiritual.prime-$Version$channelSuffix-full.nupkg"),
    (Join-Path $releaseDir "dev.spiiritual.prime-$Channel-Setup.exe"),
    (Join-Path $releaseDir "dev.spiiritual.prime-$Channel-Portable.zip"),
    (Join-Path $releaseDir "releases.$Channel.json"),
    (Join-Path $releaseDir "RELEASES$channelSuffix"),
    (Join-Path $releaseDir "assets.$Channel.json")
)

foreach ($artifact in $requiredArtifacts) {
    if (-not (Test-Path -LiteralPath $artifact)) {
        throw "Expected Velopack artifact was not found: $artifact"
    }
}

$deltaArtifact = Join-Path $releaseDir "dev.spiiritual.prime-$Version$channelSuffix-delta.nupkg"
$releaseAssets = @($requiredArtifacts)
if (Test-Path -LiteralPath $deltaArtifact) {
    $releaseAssets += $deltaArtifact
}

Invoke-Checked git @("add", "Cargo.toml", "Cargo.lock")
Invoke-Checked git @("commit", "-m", "Release Prime $Version")
Invoke-Checked git @("tag", $tag)

if ($Publish) {
    Invoke-Checked git @("push", $remote, "HEAD")
    Invoke-Checked git @("push", $remote, $tag)

    $releaseArgs = @("release", "create", $tag)
    $releaseArgs += $releaseAssets
    $releaseArgs += @("--title", "Prime $Version", "--notes-file", $notesPath)
    if ($Draft) {
        $releaseArgs += "--draft"
    }
    if ($Prerelease) {
        $releaseArgs += "--prerelease"
    }

    Invoke-Checked gh $releaseArgs
    $releaseUrl = (Invoke-Captured gh @("release", "view", $tag, "--json", "url", "--jq", ".url")).Output
    Write-Host "Published ${tag}: $releaseUrl"
} else {
    Write-Host "Created local release commit and tag '$tag'."
    Write-Host "Use -Publish on the first run when you want this script to push and create the GitHub release."
}
