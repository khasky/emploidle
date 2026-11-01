function BrowserDocuments($root) {
    if ($null -eq ('EmploidleBrowserWindows' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class EmploidleBrowserWindows {
    private delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] private static extern bool EnumChildWindows(IntPtr window, Callback callback, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder name, int length);
    public static IntPtr[] Documents(IntPtr window) {
        var result = new List<IntPtr>();
        EnumChildWindows(window, (child, data) => {
            var name = new StringBuilder(256);
            GetClassName(child, name, name.Capacity);
            if (name.ToString() == "Chrome_RenderWidgetHostHWND") result.Add(child);
            return true;
        }, IntPtr.Zero);
        return result.ToArray();
    }
}
'@
    }
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Document)
    foreach ($window in [EmploidleBrowserWindows]::Documents([IntPtr]([long]$request.window))) {
        $element = [System.Windows.Automation.AutomationElement]::FromHandle($window)
        if ($null -eq $element) { continue }
        $document = $element.FindFirst([System.Windows.Automation.TreeScope]::Subtree, $condition)
        if ($null -eq $document) { continue }
        $value = Pattern $document ([System.Windows.Automation.ValuePattern]::Pattern)
        if ($null -ne $value -and $value.Current.Value.Length -le 4096) {
            @{ element = $document; address = [string]$value.Current.Value; title = [string]$document.Current.Name }
        }
    }
}

function ExistingBrowserTabs($root) {
    $controls = BrowserControls $root
    $documents = @(BrowserDocuments $root)
    foreach ($tab in $controls.tabs) {
        $matches = @($documents | Where-Object {
            $_.title.Length -gt 0 -and ($tab.Current.Name -eq $_.title -or $tab.Current.Name.StartsWith($_.title + ', ', [StringComparison]::Ordinal))
        })
        if ($matches.Count -ne 1) { continue }
        $sameTitle = @($controls.tabs | Where-Object { $_.Current.Name -eq $tab.Current.Name })
        if ($sameTitle.Count -ne 1) { continue }
        @{ id = @($tab.GetRuntimeId()); document = @($matches[0].element.GetRuntimeId()); address = $matches[0].address;
            selected = (Pattern $tab ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected }
    }
}

function SelectExistingBrowserTab($root) {
    CheckInput ([long]$request.foreground)
    $tabs = @(ExistingBrowserTabs $root)
    $known = $tabs | Where-Object { ($_.id -join ',') -eq ($request.tab.id -join ',') -and
        ($_.document -join ',') -eq ($request.tab.document -join ',') -and $_.address -ceq $request.tab.address } | Select-Object -First 1
    if ($null -eq $known) { throw 'The browser tab changed before selection; preserving it' }
    $controls = BrowserControls $root
    $tab = $controls.tabs | Where-Object { SameElement $_ $known.id } | Select-Object -First 1
    CheckInput ([long]$request.foreground)
    (Pattern $tab ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Select()
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        $foreground = [EmploidleDesktop]::GetForegroundWindow().ToInt64()
        if ($foreground -notin @([long]$request.foreground, [long]$request.window)) { throw 'Window selection was interrupted' }
        CheckInput $foreground
        $fresh = BrowserControls $root
        if (SameElement $fresh.selected $known.id) { Respond $true; return }
        Start-Sleep -Milliseconds 15
    } while ($timer.ElapsedMilliseconds -lt 700)
    throw 'The requested browser tab did not become selected'
}

function CheckReadingInput {
    CheckInput
    CheckReadingPage
    if ($script:readingCancellation.IsCompleted) { throw 'Reading was interrupted' }
}

function CheckReadingPage {
    if ($script:readingAddress.Current.Value -cne $request.address) { throw 'The browser page changed; preserving the current view' }
}

function ReadingPositionMatches($current, $expected) {
    return [Math]::Abs($current.horizontal - $expected.horizontal) -lt 0.01 -and [Math]::Abs($current.vertical - $expected.vertical) -lt 0.01
}

function ReviewVisibleText($document) {
    $focused = [System.Windows.Automation.AutomationElement]::FocusedElement
    if ($null -ne $focused -and $focused.Current.ControlType -in @([System.Windows.Automation.ControlType]::Edit, [System.Windows.Automation.ControlType]::ComboBox)) { return }
    $pattern = Pattern $document ([System.Windows.Automation.TextPattern]::Pattern)
    if ($null -eq $pattern -or $pattern.SupportedTextSelection -eq [System.Windows.Automation.SupportedTextSelection]::None) { return }
    $old = @($pattern.GetSelection())
    $start = [System.Windows.Automation.Text.TextPatternRangeEndpoint]::Start
    $end = [System.Windows.Automation.Text.TextPatternRangeEndpoint]::End
    if ($old.Count -gt 1 -or ($old.Count -eq 1 -and $old[0].CompareEndpoints($start, $old[0], $end) -ne 0)) { return }
    $visible = @($pattern.GetVisibleRanges())
    if ($visible.Count -eq 0) { return }
    $selection = $visible[0].Clone()
    $selection.MoveEndpointByRange($end, $selection, $start)
    $null = $selection.MoveEndpointByUnit($end, [System.Windows.Automation.Text.TextUnit]::Character, 100)
    if ($selection.CompareEndpoints($end, $visible[0], $end) -gt 0) { $selection.MoveEndpointByRange($end, $visible[0], $end) }
    if ($selection.CompareEndpoints($start, $selection, $end) -eq 0) { return }
    $restore = if ($old.Count -eq 1) { $old[0] } else { $visible[0].Clone() }
    if ($old.Count -eq 0) { $restore.MoveEndpointByRange($end, $restore, $start) }
    CheckReadingInput
    $ownedSelection = $selection.Clone()
    try {
        $selection.Select()
        $timer = [Diagnostics.Stopwatch]::StartNew()
        do {
            CheckReadingInput
            $actual = @($pattern.GetSelection())
            CheckReadingInput
            if ($actual.Count -eq 1 -and $actual[0].CompareEndpoints($start, $actual[0], $end) -ne 0) { $ownedSelection = $actual[0].Clone(); break }
            Start-Sleep -Milliseconds 15
        } while ($timer.ElapsedMilliseconds -lt 700)
        for ($step = 0; $step -lt 12; $step++) { CheckReadingInput; Start-Sleep -Milliseconds 30 }
    } catch {
        if ($_.Exception.Message -notmatch 'interrupted|no longer active') { throw }
    } finally {
        $current = @($pattern.GetSelection())
        if ($null -ne $ownedSelection -and $current.Count -eq 1 -and $current[0].Compare($ownedSelection)) { $restore.Select() }
    }
}

function WaitReadingPercent($document, $pattern, [double]$target, [double]$lower, [double]$upper, [uint32]$tick, [double]$tolerance, [bool]$monitorCancellation) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        CheckReadingPage
        if ([EmploidleDesktop]::InputTick() -ne $tick) { throw 'Input changed during scrolling; preserving the current view' }
        if ($monitorCancellation) { CheckReadingInput }
        $current = ScrollPosition $document $pattern
        if ($current.vertical -lt $lower - $tolerance -or $current.vertical -gt $upper + $tolerance) { throw 'Scroll position changed outside reading; preserving the current view' }
        if ([Math]::Abs($current.vertical - $target) -le $tolerance) { return $current }
        Start-Sleep -Milliseconds 15
    } while ($timer.ElapsedMilliseconds -lt 700)
    throw "The browser did not confirm its scroll position (requested $target, reported $($current.vertical))"
}

function ReadBrowserPage($root) {
    CheckInput
    $tab = @(ExistingBrowserTabs $root | Where-Object { $_.selected -and $_.address -ceq $request.address })
    if ($tab.Count -ne 1) { throw 'The selected browser page changed before reading' }
    $documents = @(BrowserDocuments $root)
    $document = ($documents | Where-Object { SameElement $_.element $tab[0].document } | Select-Object -First 1).element
    if ($null -eq $document) { throw 'The selected browser document is unavailable' }
    $script:readingAddress = Pattern $document ([System.Windows.Automation.ValuePattern]::Pattern)
    if ($null -eq ('EmploidleReadingEvents' -as [type])) {
        Add-Type -ReferencedAssemblies @([System.Windows.Automation.AutomationElement].Assembly.Location, [System.Windows.Automation.AutomationProperty].Assembly.Location, [System.Windows.Rect].Assembly.Location) -TypeDefinition @"
public static class EmploidleReadingEvents {
    private static readonly System.Windows.Automation.AutomationPropertyChangedEventHandler Scroll = delegate(object sender, System.Windows.Automation.AutomationPropertyChangedEventArgs args) {};
    private static readonly System.Windows.Automation.AutomationEventHandler Selection = delegate(object sender, System.Windows.Automation.AutomationEventArgs args) {};
    public static void Start(System.Windows.Automation.AutomationElement element) {
        System.Windows.Automation.Automation.AddAutomationPropertyChangedEventHandler(element, System.Windows.Automation.TreeScope.Subtree, Scroll,
            System.Windows.Automation.ScrollPattern.VerticalScrollPercentProperty, System.Windows.Automation.ScrollPattern.HorizontalScrollPercentProperty,
            System.Windows.Automation.ValuePattern.ValueProperty);
        System.Windows.Automation.Automation.AddAutomationEventHandler(System.Windows.Automation.TextPattern.TextSelectionChangedEvent, element, System.Windows.Automation.TreeScope.Subtree, Selection);
    }
    public static void Stop(System.Windows.Automation.AutomationElement element) {
        System.Windows.Automation.Automation.RemoveAutomationPropertyChangedEventHandler(element, Scroll);
        System.Windows.Automation.Automation.RemoveAutomationEventHandler(System.Windows.Automation.TextPattern.TextSelectionChangedEvent, element, Selection);
    }
}
"@
    }
    [EmploidleReadingEvents]::Start($document)
    try { ReadBrowserDocument $document } finally { [EmploidleReadingEvents]::Stop($document) }
}

function ReadBrowserDocument($document) {
    $candidates = $document.FindAll([System.Windows.Automation.TreeScope]::Subtree,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsScrollPatternAvailableProperty, $true))
    $largest = 0
    $scrollDocument = $null
    $pattern = $null
    foreach ($candidate in $candidates) {
        if ($candidate.Current.IsOffscreen) { continue }
        $scroll = Pattern $candidate ([System.Windows.Automation.ScrollPattern]::Pattern)
        $bounds = $candidate.Current.BoundingRectangle
        $area = $bounds.Width * $bounds.Height
        if ($null -ne $scroll -and $scroll.Current.VerticallyScrollable -and $area -gt $largest) {
            $scrollDocument = $candidate; $pattern = $scroll; $largest = $area
        }
    }
    if ($null -eq $pattern -or -not $pattern.Current.VerticallyScrollable) {
        $script:readingCancellation = [EmploidleDesktop]::ReadCancellation()
        if ($request.select_text) { ReviewVisibleText $document }
        Respond $true; return
    }
    $document = $scrollDocument
    $before = ScrollPosition $document $pattern
    if ([double]::IsNaN($before.vertical) -or [double]::IsInfinity($before.vertical) -or $before.vertical -lt 0 -or $before.vertical -gt 100) { throw 'The document has no valid vertical scroll position' }
    @{ ok = $true; recovery = @{ before = $before; after = $before }; commit_required = $true } | ConvertTo-Json -Depth 12 -Compress | ForEach-Object { [Console]::Out.WriteLine($_) }
    if ([Console]::In.ReadLine() -ne 'commit') { Respond $true; return }
    $script:readingCancellation = [EmploidleDesktop]::ReadCancellation()
    if ($request.select_text) {
        try { $null = ReviewVisibleText $document } finally {
            CheckReadingPage
            if ([EmploidleDesktop]::InputTick() -ne [uint32]$request.input_tick) { throw 'Input changed during text review; preserving the current view' }
            $selectionAfter = ScrollPosition $document $pattern
            RestoreScroll $document $pattern $before $selectionAfter
        }
        Respond $true; return
    }
    $after = $before
    $requested = $before.vertical
    $viewSize = $pattern.Current.VerticalViewSize
    $scrollPixels = [Math]::Max(1, $before.bounds.height * (100 / [Math]::Max(0.001, $viewSize) - 1))
    $tolerance = [Math]::Max(0.01, 100 / $scrollPixels + 0.001)
    try {
        $timer = [Diagnostics.Stopwatch]::StartNew()
        while ($requested -lt 100 -and $timer.ElapsedMilliseconds -lt 14000) {
            CheckReadingInput
            $current = ScrollPosition $document $pattern
            # Chrome publishes accessibility scroll offsets asynchronously while our ramp is in flight.
            if ([Math]::Abs($current.horizontal - $before.horizontal) -gt 0.01 -or $current.vertical -lt $after.vertical - $tolerance -or $current.vertical -gt $requested + $tolerance) { throw 'Scroll position changed outside reading; preserving the current view' }
            $after = $current
            $step = [Math]::Max(0.2, [Math]::Min(3.0, $viewSize * 0.12))
            $requested = [Math]::Min(100, $requested + $step)
            $pattern.SetScrollPercent(-1, $requested)
            Start-Sleep -Milliseconds (Get-Random -Minimum 20 -Maximum 55)
        }
        CheckReadingInput
        $pattern.SetScrollPercent(-1, $requested)
        $after = WaitReadingPercent $document $pattern $requested $after.vertical $requested ([uint32]$request.input_tick) $tolerance $true

    } catch {
        if ($_.Exception.Message -notmatch 'interrupted|no longer active') { throw }
    } finally {
        CheckReadingPage
        $current = ScrollPosition $document $pattern
        if ([EmploidleDesktop]::InputTick() -ne [uint32]$request.input_tick -and
            (-not (ReadingPositionMatches $current $after) -or [Math]::Abs($requested - $current.vertical) -gt $tolerance)) { throw 'Input changed during reading; preserving the current view' }
        $restoreTick = [EmploidleDesktop]::InputTick()
        if ($current.vertical -lt $after.vertical - $tolerance -or $current.vertical -gt $requested + $tolerance) { throw 'Scroll position changed outside reading; preserving the current view' }
        $pattern.SetScrollPercent(-1, $requested)
        $after = WaitReadingPercent $document $pattern $requested $after.vertical $requested $restoreTick $tolerance $false
        $timer = [Diagnostics.Stopwatch]::StartNew()
        while ($requested -gt $before.vertical) {
            CheckReadingPage
            if ([EmploidleDesktop]::InputTick() -ne $restoreTick) { throw 'Input changed during restoration; preserving the current view' }
            $current = ScrollPosition $document $pattern
            if ([Math]::Abs($current.horizontal - $before.horizontal) -gt 0.01 -or $current.vertical -lt $requested - $tolerance -or $current.vertical -gt $after.vertical + $tolerance) { throw 'Scroll position changed outside reading; preserving the current view' }
            $after = $current
            $step = [Math]::Max(0.5, [Math]::Min(8.0, $viewSize * 0.3))
            $requested = if ($timer.ElapsedMilliseconds -ge 5000) { $before.vertical } else { [Math]::Max($before.vertical, $requested - $step) }
            $pattern.SetScrollPercent(-1, $requested)
            Start-Sleep -Milliseconds 15
        }
        if ([EmploidleDesktop]::InputTick() -ne $restoreTick) { throw 'Input changed during restoration; preserving the current view' }
        $pattern.SetScrollPercent(-1, [double]$before.vertical)
        $null = WaitReadingPercent $document $pattern $before.vertical $before.vertical $after.vertical $restoreTick 0.01 $false
    }
    Respond $true
}
