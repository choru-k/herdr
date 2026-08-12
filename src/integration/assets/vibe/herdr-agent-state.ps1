# installed by herdr
# managed by herdr; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# HERDR_INTEGRATION_ID=vibe
# HERDR_INTEGRATION_VERSION=1

if ($env:HERDR_ENV -ne "1") { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:HERDR_SOCKET_PATH)) { exit 0 }
if ([string]::IsNullOrWhiteSpace($env:HERDR_PANE_ID)) { exit 0 }

$inputText = [Console]::In.ReadToEnd()
try {
    if ([string]::IsNullOrWhiteSpace($inputText)) { exit 0 }
    $payload = $inputText | ConvertFrom-Json
} catch {
    exit 0
}
if ($null -eq $payload -or $payload -isnot [pscustomobject]) { exit 0 }
if ($payload.hook_event_name -ne "post_agent") { exit 0 }

$sessionId = if ($payload.session_id -is [string]) { $payload.session_id.Trim() } else { "" }
if ([string]::IsNullOrWhiteSpace($sessionId)) { exit 0 }

$parentSessionId = $payload.parent_session_id
if ($null -ne $parentSessionId -and -not [string]::IsNullOrWhiteSpace([string]$parentSessionId)) {
    exit 0
}

$transcriptPath = if ($payload.transcript_path -is [string]) {
    $payload.transcript_path.Trim()
} else {
    ""
}
$seq = [DateTime]::UtcNow.Ticks
$herdr = if ([string]::IsNullOrWhiteSpace($env:HERDR_BIN_PATH)) { "herdr" } else { $env:HERDR_BIN_PATH }
$arguments = @(
    "pane", "report-agent-session", $env:HERDR_PANE_ID,
    "--source", "herdr:vibe",
    "--agent", "vibe",
    "--seq", [string]$seq,
    "--agent-session-id", $sessionId
)
if (-not [string]::IsNullOrWhiteSpace($transcriptPath)) {
    $arguments += @("--agent-session-path", $transcriptPath)
}

$invocationJson = [pscustomobject]@{
    executable = $herdr
    arguments = $arguments
} | ConvertTo-Json -Compress
$job = $null
try {
    $job = Start-Job -ScriptBlock {
        param([string]$serializedInvocation)
        $invocation = $serializedInvocation | ConvertFrom-Json
        $executable = [string]$invocation.executable
        $commandArguments = @($invocation.arguments | ForEach-Object { [string]$_ })
        & $executable @commandArguments 2>$null | Out-Null
    } -ArgumentList $invocationJson
    $completed = Wait-Job -Job $job -Timeout 2 -ErrorAction SilentlyContinue
    if ($null -eq $completed) {
        Stop-Job -Job $job -ErrorAction SilentlyContinue | Out-Null
    }
} catch {
} finally {
    if ($null -ne $job) {
        Remove-Job -Job $job -Force -ErrorAction SilentlyContinue | Out-Null
    }
}
exit 0
