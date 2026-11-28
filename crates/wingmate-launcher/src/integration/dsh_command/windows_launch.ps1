# Wingmate command dispatcher
$ErrorActionPreference = 'Stop'

function Quote-Argument([string] $Value) {
    if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
    '"' + (($Value -replace '(\\*)"', '$1$1\"') -replace '(\\+)$', '$1$1') + '"'
}

try {
    # Windows PowerShell splits arguments such as -name:value in $args.
    $commandLine = [Environment]::GetCommandLineArgs()
    $kindIndex = [Array]::IndexOf($commandLine, '-File') + 2
    $kind = $commandLine[$kindIndex]
    if ($kind -notin @('wingmate', 'dsh')) { throw 'Unknown Wingmate command' }
    $config = Get-Content -LiteralPath (Join-Path $PSScriptRoot "$kind.json") -Raw -Encoding UTF8 | ConvertFrom-Json
    $process = New-Object System.Diagnostics.ProcessStartInfo
    $process.FileName = $config.executable
    $process.UseShellExecute = $false

    foreach ($entry in $config.environment.PSObject.Properties) {
        $process.EnvironmentVariables[$entry.Name] = $entry.Value
    }
    if ($config.tools) {
        if (-not [IO.File]::Exists($config.arguments[0])) {
            throw 'DSH runtime moved. Register dsh again in Wingmate settings.'
        }
        $primary, $fallback = $config.tools
        $paths = @($primary)
        $paths += ([string]$process.EnvironmentVariables['PATH']).Split(';') |
            Where-Object { $_ -and $_ -ne $primary -and $_ -ne $fallback }
        if ([IO.Directory]::Exists($fallback)) { $paths += $fallback }
        $process.EnvironmentVariables['PATH'] = $paths -join ';'
    }

    $arguments = @($config.arguments) + @($commandLine | Select-Object -Skip ($kindIndex + 1))
    $process.Arguments = ($arguments | ForEach-Object { Quote-Argument $_ }) -join ' '
    $child = [Diagnostics.Process]::Start($process)
    $child.WaitForExit()
    $code = $child.ExitCode
    $child.Dispose()
    exit $code
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
