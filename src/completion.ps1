Register-ArgumentCompleter -Native -CommandName mcdocker,mcdocker.exe -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $words = @('mcdocker')
    foreach ($element in ($commandAst.CommandElements | Select-Object -Skip 1)) {
        if ($element.Extent.StartOffset -ge $cursorPosition) { break }
        if ($element.Extent.EndOffset -ge $cursorPosition) { break }
        if ($element -is [System.Management.Automation.Language.StringConstantExpressionAst]) {
            $words += $element.Value
        } else {
            $words += $element.Extent.Text
        }
    }
    $words += ([string]$wordToComplete).Trim([char]39, [char]34)
    $OutputEncoding = New-Object System.Text.UTF8Encoding($false)
    $json = ConvertTo-Json -Compress -InputObject $words
    $rows = $json | mcdocker _complete | ConvertFrom-Json
    foreach ($row in $rows) {
        $value = $row.value
        $insert = $value
        if ($value -notmatch '^[\w./:\\=+-]+$') {
            $insert = "'" + $value.Replace("'", "''") + "'"
        }
        $tip = if ($row.help) { $row.help } else { $value }
        [System.Management.Automation.CompletionResult]::new($insert, $value, 'ParameterValue', $tip)
    }
}
