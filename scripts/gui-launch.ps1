<#
.SYNOPSIS
    Launch a GUI application from a non-interactive or SSH session in Windows using PsExec.

.EXAMPLE
    .\gui-launch.ps1 notepad.exe
    .\gui-launch.ps1 "C:\Program Files\SomeApp\app.exe" /flag

.OPTIONAL
    To create a shortcut function, add this to your PowerShell profile:
        function gui-launch { & "C:\Path\To\gui-launch.ps1" @args }
    Then you can call:
        gui-launch calc
        gui-launch "C:\MyApp\something.exe" /with /args
#>

param (
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$Command
)

# PsExec path
$psexec = (where.exe psexec).Split("`n")[0]

if (-not $psexec) {
    Write-Error "PsExec not found. Make sure PSTools is installed and in PATH."
    exit 1
}

# Get current working directory (from where script is run)
$currentDir = (Get-Location).Path

# Resolve any relative paths in the command to absolute paths
$Command = $Command | ForEach-Object {
    if ($_ -match '^\.\\|^\./') {
        # Remove the .\ or ./ prefix before joining paths
        $relativePath = $_ -replace '^\.\\|^\./', ''
        $fullPath = Join-Path $currentDir $relativePath
        if (Test-Path $fullPath) {
            $fullPath
        } else {
            $_
        }
    } else {
        $_
    }
}

# Get active session ID
$activeSession = (query session | Where-Object { $_ -match 'Active' } | ForEach-Object { 
    $parts = $_ -split '\s+' | Where-Object { $_ -ne '' }
    $parts[2]  # ID is the third column
})
if (-not $activeSession) {
    Write-Error "No active session found"
    exit 1
}

# Use PowerShell wrapping when there are arguments
$escapedCommand = $Command | ForEach-Object {
    if ($_ -match '\s') {
        "`"$_`""
    } else {
        $_
    }
}
$escapedCommand = $escapedCommand -join ' '
$escapedCommand = "`"$escapedCommand`""

# Create a temporary file for output
$tempFile = [System.IO.Path]::GetTempFileName()

& $psexec -accepteula -h -d -i $activeSession -u vagrant -p vagrant -w $currentDir powershell.exe -WindowStyle Hidden -Command "conhost.exe --headless $escapedCommand 2>&1 | Tee-Object -FilePath $tempFile; Add-Content '$tempFile' '__END__'" *> $null

# Function to sanitize log content by removing control characters and VT sequences
function Remove-DangerousCharacters {
    param([string]$Content)
    # Remove OSC sequences (starting with ]0;)
    $Content = $Content -replace '\]0;.*?\x07', ''
    # Remove ANSI/VT escape sequences including clear screen, cursor positioning
    $Content = $Content -replace '\x1B\[[0-9;]*[a-zA-Z]', ''
    # Remove CSI sequences (starting with ESC[)
    $Content = $Content -replace '\x1B\[[?]?[0-9;]*[a-zA-Z]', ''
    # Remove character deletion sequences (like [27X)
    $Content = $Content -replace '\[[0-9]+X', ''
    # Remove other control characters except newlines and tabs
    $Content = $Content -replace '[^\x20-\x7E\x0A\x09]', ''
    return $Content
}

# Read and display the output
if (Test-Path $tempFile) {
    try {
        Get-Content $tempFile -Wait | ForEach-Object {
            if ($_ -eq '__END__') {
                break
            }
            Remove-DangerousCharacters $_
        }
    }
    finally {
        # Clean up the temporary file
        if (Test-Path $tempFile) {
            Remove-Item $tempFile -Force
        }
    }
}
