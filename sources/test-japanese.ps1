# Isolated mocks exercise the production script without changing this PC.
$ErrorActionPreference = 'Stop'
function Get-WinSystemLocale { [pscustomobject]@{Name=$script:locale} }
function Set-WinSystemLocale($SystemLocale) { $script:locale=$SystemLocale }
function Get-ItemProperty($LiteralPath) { [pscustomobject]$script:pages.Clone() }
function Set-ItemProperty($LiteralPath,$Name,$Value) {
    if ($script:failOnce -and $Name -eq 'OEMCP') {
        $script:failOnce=$false
        throw 'Simulated write failure'
    }
    $script:pages[$Name]=$Value
}
function Restart-Computer { throw 'Unexpected reboot' }
foreach ($failure in $false,$true) {
    $script:locale='fr-FR'
    $script:pages=@{ACP='65001';OEMCP='65001';MACCP='65001'}
    $script:failOnce=$failure
    $failed=$false
    try { . (Join-Path $PSScriptRoot 'configure-japanese.ps1') } catch { $failed=$true; if (!$failure) {throw} }
    if ($failure) {
        if (!$failed -or $script:locale -ne 'fr-FR' -or ($script:pages.Values | Where-Object {$_ -ne '65001'})) {
            throw 'Rollback did not restore the previous configuration.'
        }
    } elseif ($failed -or $script:locale -ne 'ja-JP' -or $script:pages.ACP -ne '932' -or $script:pages.OEMCP -ne '932' -or $script:pages.MACCP -ne '10001') {
        throw 'Japanese configuration did not complete.'
    }
}
Write-Output 'Japanese configuration and rollback checks passed.'
