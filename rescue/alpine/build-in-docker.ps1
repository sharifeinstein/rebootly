<#
.SYNOPSIS
  Builds the Rebootly rescue ISO with Docker Desktop (runs build.sh in an
  Alpine container). Results go to dist\.

.EXAMPLE
  .\rescue\alpine\build-in-docker.ps1

.PARAMETER Release
  Alpine release to build on (default 3.22).
#>
param([string]$Release = "3.22")
$ErrorActionPreference = "Stop"
$repo = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "Docker isn't installed (Docker Desktop, with Linux containers). Or run build.sh in WSL."
}
docker run --rm -t `
    -v "${repo}:/src" -w /src `
    -v rebootly-cargo:/root/.cargo -v rebootly-rustup:/root/.rustup `
    -e "ALPINE_RELEASE=$Release" `
    "alpine:$Release" sh rescue/alpine/build.sh --release $Release
if ($LASTEXITCODE -ne 0) { throw "the build failed (exit code $LASTEXITCODE)" }
Get-ChildItem (Join-Path $repo "dist") -Filter "rebootly-rescue-*"
