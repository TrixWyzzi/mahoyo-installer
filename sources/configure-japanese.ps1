$previousLocale = Get-WinSystemLocale
$key = 'HKLM:\SYSTEM\CurrentControlSet\Control\Nls\CodePage'
$previousCodePages = Get-ItemProperty -LiteralPath $key
try {
    Set-WinSystemLocale -SystemLocale ja-JP
    foreach ($entry in @{ ACP = '932'; OEMCP = '932'; MACCP = '10001' }.GetEnumerator()) {
        Set-ItemProperty -LiteralPath $key -Name $entry.Key -Value $entry.Value
    }
    if ((Get-WinSystemLocale).Name -ne 'ja-JP') { throw "La langue système japonaise est absente." }
    $actual = Get-ItemProperty -LiteralPath $key
    if ($actual.ACP -ne '932' -or $actual.OEMCP -ne '932' -or $actual.MACCP -ne '10001') {
        throw "Les pages de codes japonaises sont incorrectes."
    }
} catch {
    $failure = $_
    try {
        Set-WinSystemLocale -SystemLocale $previousLocale.Name
        foreach ($name in 'ACP','OEMCP','MACCP') {
            Set-ItemProperty -LiteralPath $key -Name $name -Value $previousCodePages.$name
        }
    } catch { throw "Configuration incomplète. Ouvrez les paramètres régionaux pour vérifier le réglage. $failure $_" }
    throw $failure
}
