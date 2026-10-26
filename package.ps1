$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $MyInvocation.MyCommand.Path

$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $candidate = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path -LiteralPath $candidate) { $cargo = $candidate }
}
if (-not $cargo) { throw "cargo not found. Install Rust or add %USERPROFILE%\.cargo\bin to PATH." }

$vcvars = $null
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
        $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($install) {
            $candidate = Join-Path $install "VC\Auxiliary\Build\vcvars64.bat"
            if (Test-Path -LiteralPath $candidate) { $vcvars = $candidate }
        }
    }
    if (-not $vcvars) {
        $candidates = @(
            "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
            "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
            "C:\Program Files (x86)\Microsoft Visual Studio\18\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
        )
        $vcvars = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
}

Push-Location $root
try {
    if ($vcvars) {
        cmd.exe /c "call `"$vcvars`" >nul && `"$cargo`" build --release 2>&1"
    } else {
        & $cargo build --release
    }
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed with $LASTEXITCODE" }

    $cargoToml = Get-Content -LiteralPath (Join-Path $root "Cargo.toml") -Raw
    if ($cargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
        throw "could not read package version from Cargo.toml"
    }
    $version = $Matches[1]
    $releaseRoot = Join-Path $root "release"
    $outDir = Join-Path $releaseRoot "v$version"
    if (Test-Path -LiteralPath $outDir) { Remove-Item -LiteralPath $outDir -Recurse -Force }
    New-Item -ItemType Directory -Force -Path $outDir | Out-Null
    Copy-Item (Join-Path $root "target\release\singularity.exe") (Join-Path $outDir "Singularity.exe") -Force

    $zip = Join-Path $outDir "Singularity-win64.zip"
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }
    $staging = Join-Path $env:TEMP ("singularity-package-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $staging | Out-Null
    try {
        Copy-Item (Join-Path $outDir "Singularity.exe") (Join-Path $staging "Singularity.exe") -Force
        Copy-Item (Join-Path $root "README.md") (Join-Path $staging "README.md") -Force
        $stagingDocs = Join-Path $staging "docs"
        New-Item -ItemType Directory -Force -Path $stagingDocs | Out-Null
        Copy-Item (Join-Path $root "docs\code_specification.md") (Join-Path $stagingDocs "code_specification.md") -Force
        $stagingAssets = Join-Path $staging "assets"
        New-Item -ItemType Directory -Force -Path $stagingAssets | Out-Null
        Copy-Item (Join-Path $root "assets\preview.png") (Join-Path $stagingAssets "preview.png") -Force
        Compress-Archive -Path (Join-Path $staging "*") -DestinationPath $zip
    }
    finally {
        if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
    }

    Get-Item -LiteralPath (Join-Path $outDir "Singularity.exe"), $zip |
        Select-Object FullName, Length, LastWriteTime
}
finally {
    Pop-Location
}
