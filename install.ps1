# imlec installer for Windows.
#   irm https://raw.githubusercontent.com/koinkafasi/imlec/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'

$Repo    = 'koinkafasi/imlec'
$Target  = Join-Path $env:LOCALAPPDATA 'imlec'
$Startup = [Environment]::GetFolderPath('Startup')

function Info($msg) { Write-Host "==> $msg" -ForegroundColor Cyan }
function Warn($msg) { Write-Host "warn $msg" -ForegroundColor Yellow }

Info "installing imlec to $Target"
New-Item -ItemType Directory -Force -Path $Target | Out-Null

$asset = $null
try {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $asset = $release.assets | Where-Object { $_.name -eq 'imlec-x86_64-windows.zip' } | Select-Object -First 1
} catch {
    Warn "could not reach the GitHub releases API: $($_.Exception.Message)"
}

if ($asset) {
    $zip = Join-Path $env:TEMP 'imlec.zip'
    Info "downloading $($asset.browser_download_url)"
    Invoke-WebRequest $asset.browser_download_url -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath $Target -Force
    Remove-Item $zip -Force
} else {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw "No release binary found and cargo is not installed. Install Rust from https://rustup.rs and rerun."
    }
    Warn "no prebuilt release found, building from source"
    $src = Join-Path $env:TEMP 'imlec-src'
    if (Test-Path $src) { Remove-Item $src -Recurse -Force }
    git clone --depth 1 "https://github.com/$Repo.git" $src
    Push-Location $src
    try {
        cargo build --release --bin imlec
        Copy-Item 'target/release/imlec.exe' $Target -Force
        Copy-Item 'config/default.toml' $Target -Force
    } finally {
        Pop-Location
    }
}

$exe = Join-Path $Target 'imlec.exe'
if (-not (Test-Path $exe)) { throw "imlec.exe was not produced at $exe" }

# Autostart via a Startup folder shortcut. Reversible: delete the .lnk.
$shortcut = Join-Path $Startup 'imlec.lnk'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($shortcut)
$link.TargetPath = $exe
$link.WorkingDirectory = $Target
$link.WindowStyle = 7
$link.Description = 'imlec particle cursor overlay'
$link.Save()
Info "autostart shortcut written to $shortcut"

Write-Host ""
Write-Host "  imlec installed."
Write-Host ""
Write-Host "    $exe                          run it"
Write-Host "    $exe --print-config-path      where the config lives"
Write-Host "    $exe --reset-config           restore the commented defaults"
Write-Host ""
Write-Host "  Right-click the tray icon to toggle effects, open the config or exit."
Write-Host "  Remove autostart by deleting $shortcut"
Write-Host ""

Start-Process $exe
