param(
    [int]$TraceLimit = 1200,
    [int]$LeakScanLimit = 40,
    [string]$OutputDir = "docs/analysis",
    [switch]$IncludeAllTriggers
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Invoke-AdminJson {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $rawLines = & cargo run -q -p runtime -- admin @Arguments --json
    if ($LASTEXITCODE -ne 0) {
        throw "admin command failed: runtime admin $($Arguments -join ' ') --json"
    }
    $rawJson = ($rawLines -join "`n").Trim()
    if ([string]::IsNullOrWhiteSpace($rawJson)) {
        throw "admin command returned empty JSON payload: runtime admin $($Arguments -join ' ') --json"
    }

    return $rawJson | ConvertFrom-Json
}

function As-ObjectArray {
    param([object]$Value)

    if ($null -eq $Value) {
        return @()
    }
    if ($Value -is [System.Array]) {
        if ($Value.Length -eq 1 -and $Value[0] -is [System.Array]) {
            return @($Value[0])
        }
        return @($Value)
    }
    return @($Value)
}

function Get-TextOrNull {
    param([object]$Value)
    if ($null -eq $Value) {
        return $null
    }
    $text = [string]$Value
    if ([string]::IsNullOrWhiteSpace($text)) {
        return $null
    }
    return $text
}

function Get-PropertyValue {
    param(
        [object]$Object,
        [string]$Name
    )

    if ($null -eq $Object) {
        return $null
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    return $property.Value
}

function Get-ShapeCategory {
    param([string]$ModelText)
    if ([string]::IsNullOrWhiteSpace($ModelText)) {
        return "none"
    }

    $trimmed = $ModelText.Trim()
    if ($trimmed -match "```blue-lagoon-governed-actions") {
        if ($trimmed -match '"actions"\s*:') {
            return "tagged_block_actions"
        }
        return "tagged_block_noncanonical"
    }
    if ($trimmed -match "<governed-action>") {
        return "xml_wrapper"
    }
    if ($trimmed -match '"governed-action"\s*:') {
        return "tool_wrapper_json"
    }
    if ($trimmed -match '^[a-z_]+$') {
        return "bare_action_token"
    }
    if ($trimmed.StartsWith("{") -and $trimmed -match '"actions"\s*:') {
        return "untagged_actions_json"
    }
    if ($trimmed.StartsWith("{") -and $trimmed -match '"payload"\s*:') {
        return "untagged_payload_json"
    }
    return "plain_text_or_other"
}

function Get-MalformedReasonCategory {
    param([string]$LikelyCause)
    if ([string]::IsNullOrWhiteSpace($LikelyCause)) {
        return "unknown"
    }

    $lowered = $LikelyCause.ToLowerInvariant()
    if ($lowered.Contains('missing field `actions`')) {
        return "missing_actions_envelope"
    }
    if ($lowered.Contains('missing field `proposal_id`')) {
        return "missing_proposal_id"
    }
    if ($lowered.Contains('missing field `artifact_kind`')) {
        return "missing_required_payload_field"
    }
    if ($lowered.Contains("unknown variant")) {
        return "unknown_enum_variant"
    }
    if ($lowered.Contains("without the required governed-action block")) {
        return "missing_required_block"
    }
    if ($lowered.Contains("outside the required governed-action block")) {
        return "untagged_payload_outside_block"
    }
    if ($lowered.Contains("malformed or incomplete")) {
        return "malformed_or_incomplete_block"
    }
    if ($lowered.Contains("invalid governed-action proposal block")) {
        return "invalid_block_other"
    }
    return "other"
}

function Get-ControlMarkerKind {
    param([string]$Text)
    if ([string]::IsNullOrWhiteSpace($Text)) {
        return $null
    }
    $value = $Text.ToLowerInvariant()
    if ($value.Contains("<governed-action>")) {
        return "xml_wrapper"
    }
    if ($value.Contains("```blue-lagoon-governed-actions")) {
        return "tagged_block"
    }
    if ($value.Contains('"governed-action"')) {
        return "json_wrapper"
    }
    return $null
}

function Get-ControlLeakRecord {
    param([string]$TraceId)

    $report = Invoke-AdminJson -Arguments @("trace", "show", "--trace-id", $TraceId)
    foreach ($node in $report.nodes) {
        if ($node.node_kind -ne "episode_message") {
            continue
        }
        $role = Get-TextOrNull $node.payload.message_role
        if ($role -ne "assistant") {
            continue
        }
        $textBody = Get-TextOrNull $node.payload.text_body
        $marker = Get-ControlMarkerKind -Text $textBody
        if ($null -ne $marker) {
            return [PSCustomObject]@{
                trace_id = $TraceId
                marker_kind = $marker
                node_id = [string]$node.node_id
            }
        }
    }
    return $null
}

function New-CountTable {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [object[]]$Items,
        [Parameter(Mandatory = $true)]
        [string]$KeyProperty
    )

    return $Items |
        Group-Object -Property $KeyProperty |
        Sort-Object Count -Descending |
        ForEach-Object {
            [PSCustomObject]@{
                key = if ([string]::IsNullOrWhiteSpace([string]$_.Name)) { "<none>" } else { [string]$_.Name }
                count = [int]$_.Count
            }
        }
}

function New-RateTable {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [object[]]$AllItems,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [object[]]$FailureItems,
        [Parameter(Mandatory = $true)]
        [string]$KeyProperty
    )

    $allGroups = $AllItems | Group-Object -Property $KeyProperty
    $rows = @()
    foreach ($group in $allGroups) {
        $key = if ([string]::IsNullOrWhiteSpace([string]$group.Name)) { "<none>" } else { [string]$group.Name }
        $total = [int]$group.Count
        $failed = [int](@($FailureItems | Where-Object {
                    $value = [string]$_.$KeyProperty
                    if ([string]::IsNullOrWhiteSpace($value)) {
                        $value = "<none>"
                    }
                    $value -eq $key
                }).Count)
        $rate = if ($total -eq 0) { 0.0 } else { [Math]::Round(($failed / $total), 4) }
        $rows += [PSCustomObject]@{
            key = $key
            total = $total
            malformed = $failed
            malformed_rate = $rate
        }
    }
    return $rows | Sort-Object `
        @{ Expression = "malformed_rate"; Descending = $true }, `
        @{ Expression = "malformed"; Descending = $true }, `
        @{ Expression = "total"; Descending = $true }
}

function Get-Excerpt {
    param(
        [string]$Text,
        [int]$MaxLength = 220
    )

    if ([string]::IsNullOrWhiteSpace($Text)) {
        return $null
    }
    $trimmed = $Text.Trim().Replace("`r", " ").Replace("`n", " ")
    if ($trimmed.Length -le $MaxLength) {
        return $trimmed
    }
    return $trimmed.Substring(0, $MaxLength) + "..."
}

Write-Host "Collecting recent traces (limit=$TraceLimit)..."
$recent = @(As-ObjectArray (Invoke-AdminJson -Arguments @("trace", "recent", "--limit", $TraceLimit)))
if ($recent.Count -eq 0) {
    throw "no traces returned from runtime admin trace recent"
}

$selectedRecent = @(if ($IncludeAllTriggers) {
        $recent
    } else {
        $recent | Where-Object { $_.latest_trigger_kind -eq "telegram_pending_ingress" }
    })
if ($selectedRecent.Count -eq 0) {
    throw "no traces matched current selection criteria (include_all_triggers=$IncludeAllTriggers)"
}

Write-Host "Selected traces for explain pass: $($selectedRecent.Count)"

$records = @()
$position = 0
foreach ($trace in $selectedRecent) {
    $position++
    $traceId = [string]$trace.trace_id
    Write-Host "[$position/$($selectedRecent.Count)] explain trace $traceId"
    $explain = Invoke-AdminJson -Arguments @(
        "trace",
        "explain",
        "--trace-id",
        $traceId,
        "--focus",
        "failing-model-call"
    )

    $diagnosis = $explain.diagnosis
    $focusNode = $explain.focus.resolved_node
    $payload = if ($null -ne $focusNode) { Get-PropertyValue -Object $focusNode -Name "payload" } else { $null }
    $requestPayload = Get-PropertyValue -Object $payload -Name "request_payload_json"
    $metrics = Get-PropertyValue -Object $requestPayload -Name "prompt_metrics"
    $responsePayload = Get-PropertyValue -Object $payload -Name "response_payload_json"
    $responseOutput = Get-PropertyValue -Object $responsePayload -Name "output"
    $modelText = Get-TextOrNull (Get-PropertyValue -Object $responseOutput -Name "text")
    $likelyCause = Get-TextOrNull $diagnosis.likely_cause
    $failureClass = Get-TextOrNull $diagnosis.failure_class
    $malformedReason = if ($failureClass -eq "malformed_action_proposal") {
        Get-MalformedReasonCategory -LikelyCause $likelyCause
    } else {
        $null
    }

    $contextScenario = if ($null -ne $metrics) {
        Get-TextOrNull (Get-PropertyValue -Object $metrics -Name "context_scenario")
    } else {
        $null
    }
    $schemaDisclosure = if ($null -ne $metrics) {
        Get-TextOrNull (Get-PropertyValue -Object $metrics -Name "schema_disclosure")
    } else {
        $null
    }

    $records += [PSCustomObject]@{
        trace_id = $traceId
        trigger_kind = Get-TextOrNull $trace.latest_trigger_kind
        latest_status = Get-TextOrNull $trace.latest_status
        verdict = Get-TextOrNull $diagnosis.verdict
        failure_class = $failureClass
        likely_cause = $likelyCause
        malformed_reason = $malformedReason
        context_scenario = $contextScenario
        schema_disclosure = $schemaDisclosure
        model = if ($null -ne $payload) { Get-TextOrNull (Get-PropertyValue -Object $payload -Name "model") } else { $null }
        output_shape = Get-ShapeCategory -ModelText $modelText
        has_task_list_prefix = if ($null -ne $modelText) { $modelText.Contains("task_list:") } else { $false }
        model_text_excerpt = Get-Excerpt -Text $modelText
    }
}

$recordCount = $records.Count
$malformed = @(As-ObjectArray ($records | Where-Object { $_.failure_class -eq "malformed_action_proposal" }))
$malformedCount = $malformed.Count
$malformedRate = if ($recordCount -eq 0) { 0.0 } else { [Math]::Round(($malformedCount / $recordCount), 4) }

$shapeCounts = @(New-CountTable -Items $records -KeyProperty "output_shape")
$reasonCounts = if ($malformedCount -eq 0) {
    @()
} else {
    @(New-CountTable -Items $malformed -KeyProperty "malformed_reason")
}
$failureCounts = @(New-CountTable -Items $records -KeyProperty "failure_class")
$verdictCounts = @(New-CountTable -Items $records -KeyProperty "verdict")
$schemaRates = @(New-RateTable -AllItems $records -FailureItems $malformed -KeyProperty "schema_disclosure")
$scenarioRates = @(New-RateTable -AllItems $records -FailureItems $malformed -KeyProperty "context_scenario")

$prefixedIdMalformed = @(As-ObjectArray ($malformed | Where-Object { $_.has_task_list_prefix }))
$idMismatchSignalCount = $prefixedIdMalformed.Count

$candidateLeakTraces = @(As-ObjectArray ($records |
    Where-Object { $_.verdict -eq "succeeded" -and $_.trigger_kind -eq "telegram_pending_ingress" } |
    Select-Object -First $LeakScanLimit))
$leaks = @()
foreach ($candidate in $candidateLeakTraces) {
    Write-Host "Leak-scan trace $($candidate.trace_id)"
    $leakRecord = Get-ControlLeakRecord -TraceId $candidate.trace_id
    if ($null -ne $leakRecord) {
        $leaks += $leakRecord
    }
}

$distinctModels = @(As-ObjectArray ($records |
    Where-Object { -not [string]::IsNullOrWhiteSpace($_.model) } |
    Select-Object -ExpandProperty model -Unique))

$result = [PSCustomObject]@{
    generated_at_utc = [DateTime]::UtcNow.ToString("o")
    trace_limit = $TraceLimit
    include_all_triggers = [bool]$IncludeAllTriggers
    recent_trace_count = $recent.Count
    selected_trace_count = $selectedRecent.Count
    sample_trace_count = $recordCount
    malformed_trace_count = $malformedCount
    malformed_rate = $malformedRate
    leak_scan = [PSCustomObject]@{
        candidate_trace_count = $candidateLeakTraces.Count
        leak_count = $leaks.Count
        leaks = $leaks
    }
    model_sensitivity = [PSCustomObject]@{
        distinct_models = $distinctModels
        distinct_model_count = $distinctModels.Count
        cross_model_inference = if ($distinctModels.Count -le 1) { "inconclusive_single_model_sample" } else { "multi_model_sample_present" }
    }
    counts = [PSCustomObject]@{
        verdict = $verdictCounts
        failure_class = $failureCounts
        output_shape = $shapeCounts
        malformed_reason = $reasonCounts
    }
    rates = [PSCustomObject]@{
        malformed_by_schema_disclosure = $schemaRates
        malformed_by_context_scenario = $scenarioRates
    }
    signals = [PSCustomObject]@{
        malformed_with_prefixed_task_list_id = $idMismatchSignalCount
    }
    records = $records
}

if (-not (Test-Path -LiteralPath $OutputDir)) {
    New-Item -ItemType Directory -Path $OutputDir | Out-Null
}

$jsonPath = Join-Path $OutputDir "governed-action-falsification-latest.json"
$mdPath = Join-Path $OutputDir "governed-action-falsification-latest.md"

$result | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $jsonPath -NoNewline

$summaryLines = @()
$summaryLines += "# Governed-Action Falsification Summary"
$summaryLines += ""
$summaryLines += "- Generated at (UTC): $($result.generated_at_utc)"
$summaryLines += "- Sample trace count: $($result.sample_trace_count)"
$summaryLines += "- Malformed trace count: $($result.malformed_trace_count)"
$summaryLines += "- Malformed rate: $($result.malformed_rate)"
$summaryLines += "- Leak scan: $($result.leak_scan.leak_count) / $($result.leak_scan.candidate_trace_count)"
$summaryLines += "- Distinct models in sample: $($result.model_sensitivity.distinct_model_count)"
$summaryLines += "- Cross-model inference: $($result.model_sensitivity.cross_model_inference)"
$summaryLines += ""
$summaryLines += "## Failure Class Counts"
$summaryLines += ""
foreach ($row in $failureCounts) {
    $summaryLines += "- $($row.key): $($row.count)"
}
$summaryLines += ""
$summaryLines += "## Malformed Reason Counts"
$summaryLines += ""
foreach ($row in $reasonCounts) {
    $summaryLines += "- $($row.key): $($row.count)"
}
$summaryLines += ""
$summaryLines += "## Malformed Rate By Schema Disclosure"
$summaryLines += ""
foreach ($row in $schemaRates) {
    $summaryLines += "- $($row.key): malformed=$($row.malformed) total=$($row.total) rate=$($row.malformed_rate)"
}
$summaryLines += ""
$summaryLines += "## Malformed Rate By Context Scenario"
$summaryLines += ""
foreach ($row in $scenarioRates) {
    $summaryLines += "- $($row.key): malformed=$($row.malformed) total=$($row.total) rate=$($row.malformed_rate)"
}
$summaryLines += ""
$summaryLines += "## Control Leak Traces"
$summaryLines += ""
if ($leaks.Count -eq 0) {
    $summaryLines += "- none detected in scanned completed telegram traces"
} else {
    foreach ($leak in $leaks) {
        $summaryLines += "- trace=$($leak.trace_id) marker=$($leak.marker_kind) node=$($leak.node_id)"
    }
}
$summaryLines += ""
$summaryLines += "## Identifier-Mismatch Signal"
$summaryLines += ""
$summaryLines += ('- malformed outputs containing `task_list:` prefixes: {0}' -f $idMismatchSignalCount)

$summaryLines | Set-Content -LiteralPath $mdPath

Write-Host ""
Write-Host "Falsification artifact written:"
Write-Host "  - $jsonPath"
Write-Host "  - $mdPath"
Write-Host ""
Write-Host "Top malformed reasons:"
foreach ($row in ($reasonCounts | Select-Object -First 5)) {
    Write-Host "  $($row.key): $($row.count)"
}
