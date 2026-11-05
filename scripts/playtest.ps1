param(
    [string]$Map = 't5:zombie_theater',
    [string]$GameType = 'zombies',
    [switch]$NoBuild,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$GameArgs
)

# A human-led playtest with an agent beside it. F8 saves a bundle under
# iw4l-artifacts/playtest/<session>/; lines appended to the inbox file run as
# console commands in the live game (docs/PLAYTEST.md).
$ErrorActionPreference = 'Stop'
$workspaceRoot = Split-Path -Parent $PSScriptRoot
$artifacts = Join-Path $workspaceRoot 'iw4l-artifacts'
$inbox = Join-Path $artifacts 'inbox.txt'
$previous = @{
    Artifacts = $env:IW4L_ARTIFACTS_DIR
    Inbox     = $env:IW4L_CONSOLE_INBOX
    GameType  = $env:IW4L_GAMETYPE
}
Push-Location -LiteralPath $workspaceRoot
try {
    if (!$NoBuild) {
        & cargo build --locked --profile play -p launcher
        if ($LASTEXITCODE -ne 0) { throw 'The optimized game build failed.' }
    }
    New-Item -ItemType Directory -Force -Path $artifacts | Out-Null
    $env:IW4L_ARTIFACTS_DIR = $artifacts
    $env:IW4L_CONSOLE_INBOX = $inbox
    $env:IW4L_GAMETYPE = $GameType
    Write-Host "Inbox: $inbox"
    Write-Host 'F8 in game marks a problem; the console is the backtick key.'
    & (Join-Path $workspaceRoot 'target/play/iw4l.exe') map $Map @GameArgs
    $exitCode = $LASTEXITCODE
    $marks = Get-ChildItem -Path (Join-Path $artifacts 'playtest') -Recurse -Filter note.txt -ErrorAction SilentlyContinue
    if ($marks) {
        Write-Host "`nMarks this and earlier sessions:"
        $marks | Sort-Object LastWriteTime | Select-Object -Last 20 | ForEach-Object { Write-Host "  $($_.DirectoryName)" }
    }
} finally {
    $env:IW4L_ARTIFACTS_DIR = $previous.Artifacts
    $env:IW4L_CONSOLE_INBOX = $previous.Inbox
    $env:IW4L_GAMETYPE = $previous.GameType
    Pop-Location
}
exit $exitCode
