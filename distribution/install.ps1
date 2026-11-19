# Installs remuda-core (rc) on Windows, or updates it:
#
#   powershell -c "irm https://releases.herdr.dev/core/install.ps1 | iex"
#
# Into %LOCALAPPDATA%\Programs\remuda-core (remuda-core.exe, Microsoft's
# ConPTY beside it), added to your PATH, with rc.exe linked to it only if no
# `rc` exists. An rc that is already there is swapped the planned way (rc
# update --from): running terminals keep running. No admin needed.
# RC_VERSION=0.2.0 picks a version; RC_RELEASES_URL points elsewhere
# (tests); RC_INSTALL_DIR installs elsewhere.
#
# `-File install.ps1 <release zip> [session]` installs from a zip already
# here, checked by whoever put it here, and removes both files after
# (`rc machine add` from another machine does that).

# Its own scope: under `irm | iex` it would otherwise set the caller's variables.
& {
$zip, $session = $args
if ($session) { $env:RC_SESSION = $session }
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# throw, not exit: under `irm | iex`, exit would close the person's window.
function Fail($why) { throw "rc: $why" }

$releases = if ($env:RC_RELEASES_URL) { $env:RC_RELEASES_URL.TrimEnd("/") } else { "https://releases.herdr.dev" }
$dir = if ($env:RC_INSTALL_DIR) { $env:RC_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\remuda-core" }
$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($arch -ne "AMD64") { Fail "no build for $arch Windows yet (x64 only)" }
$target = "windows-x86_64"
# Windows' own tar opens zips; Git Bash's, often first on PATH, can't.
$tar = Join-Path $env:SystemRoot "System32\tar.exe"
if (-not (Get-Command curl.exe -ErrorAction SilentlyContinue) -or -not (Test-Path $tar)) {
    Fail "needs curl.exe and tar.exe (Windows 10 1803 or newer)"
}

$tmp = Join-Path ([IO.Path]::GetTempPath()) "rc-install-$PID"
New-Item -ItemType Directory -Force $tmp | Out-Null
try {
  if ($zip) {
    Copy-Item $zip "$tmp\rc.zip"
  } else {
    $manifest = if ($env:RC_VERSION) { "$releases/core/$($env:RC_VERSION)/manifest.json" } else { "$releases/core/latest.json" }
    curl.exe -fsSL $manifest -o "$tmp\manifest.json"
    if ($LASTEXITCODE) { Fail "cannot reach $manifest" }
    $m = Get-Content -Raw "$tmp\manifest.json" | ConvertFrom-Json
    $t = $m.targets.$target
    if (-not $t.url -or -not $t.sha256) { Fail "no build for $target in $manifest" }

    # Tells the release server's download counter a new install from an
    # update; the file is the same either way. Only plain URLs on the release
    # server in use are marked: one with a query or fragment (a mirror's
    # signed URL) is fetched exactly as the manifest gives it.
    $src = if (Test-Path (Join-Path $dir "remuda-core.exe")) { "update" } else { "install" }
    $plain = $t.url.StartsWith("$releases/", [StringComparison]::Ordinal) -and $t.url.IndexOfAny([char[]]"?#") -lt 0
    $download = if ($plain) { "$($t.url)?src=$src" } else { $t.url }
    curl.exe -fsSL $download -o "$tmp\rc.zip"
    if ($LASTEXITCODE) { Fail "download failed: $($t.url)" }
    # .NET, not Get-FileHash: that one fails when PowerShell 7 starts 5.1.
    $stream = [IO.File]::OpenRead("$tmp\rc.zip")
    try { $got = -join ([Security.Cryptography.SHA256]::Create().ComputeHash($stream) | ForEach-Object { $_.ToString("x2") }) } finally { $stream.Close() }
    if ($got -ne $t.sha256) { Fail "the download is damaged (checksum mismatch); try again" }
  }
    New-Item -ItemType Directory -Force "$tmp\rc" | Out-Null
    & $tar -xf "$tmp\rc.zip" -C "$tmp\rc"
    $new = "$tmp\rc\remuda-core.exe"
    if ($LASTEXITCODE -or -not (Test-Path "$tmp\rc\conpty\conpty.dll")) { Fail "the download holds no complete rc" }
    $version = (& $new --version) -replace "^rc ", ""
    if ($LASTEXITCODE) { Fail "the downloaded rc does not run on this machine" }

    New-Item -ItemType Directory -Force $dir | Out-Null
    $exe = Join-Path $dir "remuda-core.exe"
    if (Test-Path $exe) {
        # Swapped the planned way, so its terminals keep running.
        & $exe update --from $new | Out-Null
        if ($LASTEXITCODE) { Fail "updating $exe failed (see above)" }
        Write-Host "rc updated to $version."
    } else {
        Copy-Item -Recurse -Force "$tmp\rc\remuda-core.exe", "$tmp\rc\conpty" $dir
        Write-Host "rc $version installed into $dir."
    }
    Copy-Item -Recurse -Force "$tmp\rc\THIRD-PARTY-NOTICES" $dir -ErrorAction SilentlyContinue
    if (-not (Get-Command rc -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $dir "rc.exe"))) {
        New-Item -ItemType HardLink -Path (Join-Path $dir "rc.exe") -Target $exe | Out-Null
    }

    # The raw user PATH, so %VARIABLES% in it stay unexpanded and keep their type.
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
    $raw = [string]$key.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $listed = $raw -split ";" | Where-Object { $_.Trim().TrimEnd("\") -ieq $dir.TrimEnd("\") }
    if (-not $listed) {
        $kind = if ($key.GetValueNames() -contains "Path") { $key.GetValueKind("Path") } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
        $key.SetValue("Path", $(if ($raw) { "$($raw.TrimEnd(';'));$dir" } else { $dir }), $kind)
        # Setting any user variable tells running programs the environment changed.
        [Environment]::SetEnvironmentVariable("RC_INSTALLING", "1", "User")
        [Environment]::SetEnvironmentVariable("RC_INSTALLING", $null, "User")
        Write-Host "Added $dir to your PATH: new terminals have rc."
    }
    $key.Close()
    if (($env:Path -split ";") -notcontains $dir) { $env:Path += ";$dir" }
    Write-Host "Agents: rc --skill. People: rc --help."
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    if ($zip) { Remove-Item -Force $zip, $PSCommandPath -ErrorAction SilentlyContinue }
}
} @args
