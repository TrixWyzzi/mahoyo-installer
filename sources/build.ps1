$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot)
try {
    & (Join-Path $PSScriptRoot "test-japanese.ps1")
    cargo test --release --locked
    if ($LASTEXITCODE) { throw 'Les tests Rust ont échoué.' }
    cargo build --release --locked
    if ($LASTEXITCODE) { throw 'La compilation Rust a échoué.' }
    $targetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    Copy-Item -LiteralPath (Join-Path $targetDirectory 'release/mahoyo-installer.exe') -Destination 'Installer-Mahoyo.exe' -Force
} finally {
    Pop-Location
}
