param(
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidatePattern('^[A-Fa-f0-9]{40}$')][string]$SigningCertificateThumbprint,
    [switch]$AllowUnsignedPreview
)
$ErrorActionPreference = 'Stop'
$certificate = $null
if ($SigningCertificateThumbprint) {
    $certificate = Get-ChildItem Cert:/CurrentUser/My,Cert:/LocalMachine/My -CodeSigningCert |
        Where-Object Thumbprint -EQ $SigningCertificateThumbprint |
        Select-Object -First 1
    if (!$certificate -or !$certificate.HasPrivateKey -or $certificate.NotAfter -le (Get-Date)) {
        throw 'Certificat de signature valide avec clé privée introuvable.'
    }
} elseif (!$AllowUnsignedPreview) {
    throw 'Signature éditeur requise. Fournir -SigningCertificateThumbprint ou utiliser -AllowUnsignedPreview pour une préversion explicitement non signée.'
}
$root = Split-Path $PSScriptRoot
$output = [IO.Directory]::CreateDirectory($OutputDirectory).FullName
$scanner = Join-Path $env:ProgramData 'Microsoft\Windows Defender\Platform\CurrentVersion\MpCmdRun.exe'
if (!(Get-MpComputerStatus).AntivirusEnabled) { throw 'Microsoft Defender doit être actif pour contrôler la publication.' }
function Test-ReleaseFile([string]$path) {
    $hash = (Get-FileHash -LiteralPath $path).Hash
    & $scanner -Scan -ScanType 3 -File $path -DisableRemediation | Out-Host
    if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath $path)) { throw "Contrôle Defender en échec : $path" }
    if ((Get-FileHash -LiteralPath $path).Hash -ne $hash) { throw "Fichier modifié pendant le contrôle : $path" }
    return $hash
}

& (Join-Path $PSScriptRoot 'build.ps1')
$tmp = New-TemporaryFile
$zip = $tmp.FullName + '.zip'
try {
    $exe = Join-Path $root 'Installer-Mahoyo.exe'
    if ($certificate) {
        $signature = Set-AuthenticodeSignature -LiteralPath $exe -Certificate $certificate -HashAlgorithm SHA256 -IncludeChain All -TimestampServer 'http://timestamp.digicert.com'
        $signature = Get-AuthenticodeSignature -LiteralPath $exe
        if ($signature.Status -ne 'Valid' -or !$signature.TimeStamperCertificate) {
            throw 'La signature Authenticode doit être approuvée par Windows et horodatée avant publication.'
        }
    }
    $exeHash = Test-ReleaseFile $exe
    # Only the launcher, README and public manifest belong in the portable ZIP.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::Open($zip, 'Create')
    try {
        foreach ($name in 'Installer-Mahoyo.exe','README.md','donnees/manifest.tsv') {
            [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive,(Join-Path $root $name),$name) | Out-Null
        }
    } finally { $archive.Dispose() }
    $zipHash = Test-ReleaseFile $zip
    Copy-Item -LiteralPath $zip -Destination (Join-Path $output 'Mahoyo-Installateur.zip') -Force
    @("$exeHash  Installer-Mahoyo.exe","$zipHash  Mahoyo-Installateur.zip") |
        Set-Content -LiteralPath (Join-Path $output 'SHA256SUMS.txt') -Encoding ascii
} finally {
    Remove-Item -LiteralPath $tmp.FullName,$zip -Force -ErrorAction SilentlyContinue
}
