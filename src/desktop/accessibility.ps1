$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

function Pattern($element, $identifier) {
    $value = $null
    if ($element.TryGetCurrentPattern($identifier, [ref]$value)) { return $value }
    return $null
}

function ScrollPosition($element, $pattern) {
    $state = $pattern.Current
    $rectangle = $element.Current.BoundingRectangle
    return @{
        element = @($element.GetRuntimeId())
        horizontal = [double]$state.HorizontalScrollPercent
        vertical = [double]$state.VerticalScrollPercent
        bounds = @{ x = [int]$rectangle.Left; y = [int]$rectangle.Top; width = [int]$rectangle.Width; height = [int]$rectangle.Height }
    }
}

function SameElement($element, $runtimeId) {
    return [System.Windows.Automation.Automation]::Compare([int[]]$element.GetRuntimeId(), [int[]]$runtimeId)
}

function Respond($data, [bool]$commitRequired = $false) {
    @{ ok = $true; data = $data; commit_required = $commitRequired } | ConvertTo-Json -Depth 12 -Compress | ForEach-Object { [Console]::Out.WriteLine($_) }
}

function WaitScrollPosition($element, $pattern, $before, [bool]$requireChange, [bool]$monitorInput = $false) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    $stableSince = 0
    $last = ScrollPosition $element $pattern
    $changed = $false
    while ($timer.ElapsedMilliseconds -lt 700) {
        if ($monitorInput) { CheckInput }
        $position = ScrollPosition $element $pattern
        if ([Math]::Abs($position.horizontal - $before.horizontal) -gt 0.001 -or
            [Math]::Abs($position.vertical - $before.vertical) -gt 0.001) { $changed = $true }
        if ([Math]::Abs($position.horizontal - $last.horizontal) -gt 0.001 -or
            [Math]::Abs($position.vertical - $last.vertical) -gt 0.001) { $stableSince = $timer.ElapsedMilliseconds }
        if (($changed -or -not $requireChange) -and $timer.ElapsedMilliseconds - $stableSince -ge 80) { return $position }
        $last = $position
        Start-Sleep -Milliseconds 10
    }
    throw 'The scroll provider did not report a stable position'
}

function RestoreScroll($element, $pattern, $before, $after) {
    $current = ScrollPosition $element $pattern
    # A newer position belongs to the user or application, not this transaction.
    if ($null -eq $after -or [Math]::Abs($current.horizontal - $after.horizontal) -gt 0.001 -or
        [Math]::Abs($current.vertical - $after.vertical) -gt 0.001) { throw 'Scroll position changed outside this action; preserving the current view' }
    $restoreTick = [EmploidleDesktop]::InputTick()
    $pattern.SetScrollPercent([double]$before.horizontal, [double]$before.vertical)
    if ([EmploidleDesktop]::InputTick() -ne $restoreTick) { throw 'Input changed during restoration; preserving the current view' }
    $pattern.SetScrollPercent([double]$before.horizontal, [double]$before.vertical)
    $needsChange = [Math]::Abs($current.horizontal - $before.horizontal) -gt 0.001 -or [Math]::Abs($current.vertical - $before.vertical) -gt 0.001
    $restored = WaitScrollPosition $element $pattern $current $needsChange
    if ([Math]::Abs($restored.horizontal - $before.horizontal) -gt 0.01 -or
        [Math]::Abs($restored.vertical - $before.vertical) -gt 0.01) { throw 'The provider did not restore the original scroll position' }
}

function BrowserControls($root) {
    $classProperty = [System.Windows.Automation.AutomationElement]::ClassNameProperty
    $condition = [System.Windows.Automation.OrCondition]::new([System.Windows.Automation.Condition[]]@(
        [System.Windows.Automation.PropertyCondition]::new($classProperty, 'Tab'),
        [System.Windows.Automation.PropertyCondition]::new($classProperty, 'OmniboxViewViews'),
        [System.Windows.Automation.PropertyCondition]::new($classProperty, 'TabStripControlButton')))
    $elements = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $tabs = [Collections.Generic.List[object]]::new()
    $selected = $null
    $address = $null
    $newTab = $null
    for ($index = 0; $index -lt $elements.Count; $index++) {
        $element = $elements.Item($index)
        $current = $element.Current
        if ($current.ClassName -eq 'Tab' -and $current.ControlType -eq [System.Windows.Automation.ControlType]::TabItem) {
            $tabs.Add($element)
            $selection = Pattern $element ([System.Windows.Automation.SelectionItemPattern]::Pattern)
            if ($null -ne $selection -and $selection.Current.IsSelected) { $selected = $element }
        }
        if ($current.ClassName -eq 'OmniboxViewViews' -and $current.ControlType -eq [System.Windows.Automation.ControlType]::Edit) { $address = $element }
        if ($current.ClassName -eq 'TabStripControlButton' -and $current.Name -match '^(New tab|Новая вкладка|Открыть новую вкладку)(\s*\(|$)') { $newTab = $element }
    }
    if ($null -eq $selected -or $null -eq $address -or $null -eq $newTab) { throw 'This browser does not expose supported tab controls' }
    return @{ tabs = @($tabs.ToArray()); selected = $selected; address = $address; newTab = $newTab }
}

function TabCloseButton($tab) {
    $buttons = $tab.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
    foreach ($button in $buttons) {
        if ($button.Current.ClassName -eq 'TabCloseButton' -or $button.Current.Name -match '^(Close|Close tab|Закрыть|Закрыть вкладку)$') {
            $invoke = Pattern $button ([System.Windows.Automation.InvokePattern]::Pattern)
            if ($null -ne $invoke) { return $invoke }
        }
    }
    throw 'The browser does not expose an addressable tab close button'
}

function CloseOwnedTab($root, $created, $original) {
    $controls = BrowserControls $root
    $tab = $controls.tabs | Where-Object { SameElement $_ $created } | Select-Object -First 1
    if ($null -eq $tab) { return }
    $restoreSelection = (SameElement $controls.selected $created) -and
        [EmploidleDesktop]::GetForegroundWindow().ToInt64() -eq [long]$request.window
    $inputTick = [EmploidleDesktop]::InputTick()
    $close = TabCloseButton $tab
    $close.Invoke()
    if ($restoreSelection -and [EmploidleDesktop]::InputTick() -eq $inputTick -and
        [EmploidleDesktop]::GetForegroundWindow().ToInt64() -eq [long]$request.window) {
        $controls = BrowserControls $root
        $originalTab = $controls.tabs | Where-Object { SameElement $_ $original } | Select-Object -First 1
        if ($null -ne $originalTab) {
            $selection = Pattern $originalTab ([System.Windows.Automation.SelectionItemPattern]::Pattern)
            $selection.Select()
        }
    }
}

function NewBrowserTabs($controls, $previousIds) {
    foreach ($tab in $controls.tabs) {
        $known = $false
        foreach ($id in $previousIds) { if (SameElement $tab $id) { $known = $true; break } }
        if (-not $known) { $tab }
    }
}

function CheckInput([long]$foreground = [long]$request.window) {
    if ([EmploidleDesktop]::GetForegroundWindow().ToInt64() -ne $foreground) { throw 'The target window is no longer active' }
    if ($null -ne $request.input_tick -and [EmploidleDesktop]::InputTick() -ne [uint32]$request.input_tick) { throw 'The action was interrupted by input' }
}

try {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $request = [Console]::In.ReadLine() | ConvertFrom-Json
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class EmploidleDesktop {
    [StructLayout(LayoutKind.Sequential)] private struct Point { public int X; public int Y; }
    [DllImport("user32.dll")] private static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] private static extern IntPtr GetAncestor(IntPtr window, uint flags);
    [DllImport("user32.dll")] private static extern bool ScreenToClient(IntPtr window, ref Point point);
    [DllImport("user32.dll")] private static extern IntPtr SendMessageTimeout(IntPtr window, uint message, UIntPtr flags, IntPtr position, uint options, uint timeout, out UIntPtr result);
    public static void ClickClient(IntPtr root, int x, int y, uint tick) {
        var point = new Point { X = x, Y = y };
        var window = WindowFromPoint(point);
        if (GetForegroundWindow() != root || GetAncestor(window, 2) != root || InputTick() != tick)
            throw new Exception("The Explorer action was interrupted");
        if (!ScreenToClient(window, ref point)) throw new Exception("The Explorer client coordinates are unavailable");
        var position = new IntPtr(unchecked((point.Y << 16) | (point.X & 0xffff)));
        UIntPtr result;
        try {
            if (SendMessageTimeout(window, 0x201, new UIntPtr(1), position, 2, 100, out result) == IntPtr.Zero)
                throw new Exception("The Explorer click was not accepted");
        } finally {
            if (SendMessageTimeout(window, 0x202, UIntPtr.Zero, position, 2, 100, out result) == IntPtr.Zero)
                throw new Exception("The Explorer click did not finish");
        }
    }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
    public static System.Threading.Tasks.Task<string> ReadCancellation() {
        // Console.In.ReadLineAsync uses a synchronized reader and can block its caller.
        return System.Threading.Tasks.Task.Run(() => Console.In.ReadLine());
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct InputState { public uint Size; public uint Tick; }
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")]
    private static extern bool GetLastInputInfo(ref InputState state);
    public static uint InputTick() {
        var state = new InputState { Size = 8 };
        if (!GetLastInputInfo(ref state)) throw new InvalidOperationException("Cannot inspect user input");
        return state.Tick;
    }
}
'@
    $null = [EmploidleDesktop]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]([long]$request.window))
    if ($null -eq $root -or $root.Current.ProcessId -ne [int]$request.process_id -or
        ($root.Current.IsOffscreen -and $request.operation -notin @('restore', 'close_tab', 'restore_editor', 'browser_tabs', 'browser_context', 'select_tab'))) {
        throw 'The selected window is unavailable or belongs to a different process'
    }
    $rootRect = $root.Current.BoundingRectangle
    if ($request.operation -eq 'browser_tabs') { Respond @{ tabs = @(ExistingBrowserTabs $root) }; exit }
    if ($request.operation -eq 'select_tab') { SelectExistingBrowserTab $root; exit }
    if ($request.operation -eq 'read_page') { ReadBrowserPage $root; exit }
    if ($request.operation -eq 'begin_editor') { BeginEditor $root ([uint32]$request.action); exit }
    if ($request.operation -eq 'editor_file_target') { Respond (EditorFileTarget $root $request.session); exit }
    if ($request.operation -eq 'capture_editor') { CaptureEditor $root $request.session; exit }
    if ($request.operation -eq 'editor_viewport') { Respond (EditorViewport $root $request.session); exit }
    if ($request.operation -eq 'restore_editor') { RestoreEditor $root $request.session ([bool]$request.interrupted); Respond $true; exit }
    if ($request.operation -in @('browser_context', 'open_tab', 'close_tab')) {
        if ($request.operation -eq 'close_tab') {
            CloseOwnedTab $root $request.session.created $request.session.original
            Respond $true
        } else {
            $controls = BrowserControls $root
            if ($request.operation -eq 'browser_context') {
                $value = Pattern $controls.address ([System.Windows.Automation.ValuePattern]::Pattern)
                Respond @{ address = [string]$value.Current.Value; address_focused = [bool]$controls.address.Current.HasKeyboardFocus; selected = @($controls.selected.GetRuntimeId()) }
            } else {
                $original = [int[]]$controls.selected.GetRuntimeId()
                $previousIds = @($controls.tabs | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
                $null = TabCloseButton $controls.selected
                $invoke = Pattern $controls.newTab ([System.Windows.Automation.InvokePattern]::Pattern)
                if ($null -eq $invoke) { throw 'The new-tab control is not available' }
                $invoked = $false
                $owned = $null
                try {
                    CheckInput
                    $invoked = $true
                    $invoke.Invoke()
                    $timer = [Diagnostics.Stopwatch]::StartNew()
                    do {
                        CheckInput
                        $fresh = BrowserControls $root
                        $added = @(NewBrowserTabs $fresh $previousIds)
                        CheckInput
                        if ($added.Count -eq 1 -and (SameElement $fresh.selected $added[0].GetRuntimeId())) {
                            $owned = [int[]]$added[0].GetRuntimeId()
                            break
                        }
                    } while ($timer.ElapsedMilliseconds -lt 700)
                    if ($null -eq $owned) { throw 'The created browser tab could not be identified' }
                    $committed = $false
                    try {
                        Respond @{ created = @($owned); original = @($original) } $true
                        $committed = [Console]::In.ReadLine() -eq 'commit'
                    } finally {
                        if (-not $committed) { CloseOwnedTab $root $owned $original }
                    }
                } catch {
                    if ($null -ne $owned) {
                        CloseOwnedTab $root $owned $original
                    } elseif ($invoked -and $null -ne $request.input_tick -and [EmploidleDesktop]::InputTick() -eq [uint32]$request.input_tick) {
                        $fresh = BrowserControls $root
                        $added = @(NewBrowserTabs $fresh $previousIds)
                        if ($added.Count -eq 1 -and [EmploidleDesktop]::InputTick() -eq [uint32]$request.input_tick) { CloseOwnedTab $root $added[0].GetRuntimeId() $original }
                    }
                    throw
                }
            }
        }
        exit
    }
    $scope = [System.Windows.Automation.TreeScope]::Descendants
    $visible = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::IsOffscreenProperty, $false)
    $condition = if ($request.operation -eq 'restore') { [System.Windows.Automation.Condition]::TrueCondition } else { $visible }
    $elements = $root.FindAll($scope, $condition)
    if ($request.operation -eq 'review_tree') { ReviewEditorTree $root ([uint32]$request.index); exit }
    $scrollElement = $null
    $scrollPattern = $null
    $largestArea = 0
    for ($index = -1; $index -lt [Math]::Min($elements.Count, 500); $index++) {
        $element = if ($index -eq -1) { $root } else { $elements.Item($index) }
        if ($request.operation -eq 'restore') {
            if (-not (SameElement $element $request.receipt.before.element)) { continue }
        }
        $pattern = Pattern $element ([System.Windows.Automation.ScrollPattern]::Pattern)
        if ($null -eq $pattern) { continue }
        $state = $pattern.Current
        if (-not $state.VerticallyScrollable -and -not $state.HorizontallyScrollable) { continue }
        $rectangle = $element.Current.BoundingRectangle
        $area = $rectangle.Width * $rectangle.Height
        if ($area -gt $largestArea) {
            $largestArea = $area
            $scrollElement = $element
            $scrollPattern = $pattern
        }
    }
    if ($request.operation -in @('scroll', 'restore')) {
        if ($null -eq $scrollPattern) { throw 'A restorable scroll control is no longer available' }
        if ($request.operation -eq 'restore') {
            RestoreScroll $scrollElement $scrollPattern $request.receipt.before $request.receipt.after
            Respond $true
        } else {
            $before = ScrollPosition $scrollElement $scrollPattern
            foreach ($percent in @($before.horizontal, $before.vertical)) {
                if ([double]::IsNaN($percent) -or [double]::IsInfinity($percent) -or $percent -lt -1 -or $percent -gt 100) { throw 'The provider does not expose a valid scroll position' }
            }
            if (($request.horizontal -and -not $scrollPattern.Current.HorizontallyScrollable) -or
                (-not $request.horizontal -and -not $scrollPattern.Current.VerticallyScrollable)) { throw 'The requested scroll axis is unavailable' }
            if ([int]$request.amount -eq 0 -or [Math]::Abs([int]$request.amount) -gt 4) { throw 'Invalid scroll amount' }
            $direction = if ([int]$request.amount -gt 0) { [System.Windows.Automation.ScrollAmount]::SmallIncrement } else { [System.Windows.Automation.ScrollAmount]::SmallDecrement }
            $none = [System.Windows.Automation.ScrollAmount]::NoAmount
            $after = $before
            $committed = $false
            try {
                for ($step = 0; $step -lt [Math]::Abs([int]$request.amount); $step++) {
                    CheckInput
                    $position = ScrollPosition $scrollElement $scrollPattern
                    $percent = if ($request.horizontal) { $position.horizontal } else { $position.vertical }
                    $needsChange = if ([int]$request.amount -gt 0) { $percent -lt 100 } else { $percent -gt 0 }
                    if ($request.horizontal) { $scrollPattern.Scroll($direction, $none) } else { $scrollPattern.Scroll($none, $direction) }
                    $candidate = WaitScrollPosition $scrollElement $scrollPattern $position $needsChange $true
                    CheckInput
                    $after = $candidate
                }
                Respond @{ before = $before; after = $after } $true
                $committed = [Console]::In.ReadLine() -eq 'commit'
            } finally {
                if (-not $committed) { RestoreScroll $scrollElement $scrollPattern $before $after }
            }
        }
        exit
    }
    $domain = $null
    $rows = [Collections.Generic.List[object]]::new()
    $links = [Collections.Generic.List[object]]::new()
    $limit = [Math]::Min($elements.Count, 500)
    for ($index = -1; $index -lt $limit; $index++) {
        $element = if ($index -eq -1) { $root } else { $elements.Item($index) }
        $current = $element.Current
        if ($current.IsPassword -or $current.IsOffscreen) { continue }
        $rectangle = $current.BoundingRectangle
        if ($rectangle.IsEmpty -or [double]::IsInfinity($rectangle.Width) -or
        [double]::IsInfinity($rectangle.Height) -or $rectangle.Width -le 0 -or $rectangle.Height -le 0) { continue }
        # Virtualized providers can leave recycled children marked visible outside the window.
        if ($rectangle.Right -le $rootRect.Left -or $rectangle.Left -ge $rootRect.Right -or
        $rectangle.Bottom -le $rootRect.Top -or $rectangle.Top -ge $rootRect.Bottom) { continue }
        $kind = $current.ControlType.ProgrammaticName
        if ($rows.Count -lt 24 -and $kind -match '^ControlType\.(Text|ListItem|DataItem|Hyperlink|TreeItem|Button)$') {
            $left = [Math]::Max($rectangle.Left, $rootRect.Left)
            $top = [Math]::Max($rectangle.Top, $rootRect.Top)
            $right = [Math]::Min($rectangle.Right, $rootRect.Right)
            $bottom = [Math]::Min($rectangle.Bottom, $rootRect.Bottom)
            if ($right -gt $left -and $bottom -gt $top) {
                $rows.Add(@{ x = [int]$left; y = [int]$top; width = [int]($right - $left); height = [int]($bottom - $top) })
            }
        }
        $legacyType = 'System.Windows.Automation.LegacyIAccessiblePattern' -as [type]
        if ($kind -eq 'ControlType.Hyperlink' -and $links.Count -lt 24 -and $null -ne $legacyType) {
            $legacy = Pattern $element ($legacyType::Pattern)
            if ($null -ne $legacy) {
                $address = [string]$legacy.Current.Value
                if ($address -match '^https://' -and $address.Length -le 4096) {
                    $links.Add(@{ address = $address; visited = (($legacy.Current.State -band 0x800000) -ne 0) })
                }
            }
        }
        if ($kind -eq 'ControlType.Edit' -and $null -eq $domain) {
            $identifier = [string]$current.AutomationId
            $name = [string]$current.Name
            if ($current.ClassName -eq 'OmniboxViewViews' -or $identifier -match '^(addressEditBox|urlbar-input|urlbar|omnibox|location-bar|addressbar|address-field)$' -or
                $name -match '^(Address and search bar|Address bar|Search or enter address|Address and Search)$') {
                $value = Pattern $element ([System.Windows.Automation.ValuePattern]::Pattern)
                if ($null -ne $value) {
                    $address = [string]$value.Current.Value
                    if ($address.Length -le 4096) {
                        if ($address -notmatch '^https?://') { $address = 'https://' + $address }
                        $uri = $null
                        if ([Uri]::TryCreate($address, [UriKind]::Absolute, [ref]$uri) -and
                            $uri.Scheme -in @('http', 'https') -and $uri.Host.Length -le 255 -and $uri.Host.Contains('.')) { $domain = $uri.IdnHost }
                    }
                }
            }
        }
    }
    if ($rows.Count -eq 0 -and $null -eq $domain -and $null -eq $scrollPattern) {
        throw 'The selected application does not expose supported accessibility content'
    }
    $scroll = if ($null -ne $scrollPattern) { ScrollPosition $scrollElement $scrollPattern } else { $null }
    $data = @{ domain = $domain; rows = @($rows.ToArray()); scroll = $scroll; links = @($links.ToArray()) }
    Respond $data
} catch {
    $message = [string]$_.Exception.Message
    if ($message.Length -gt 500) { $message = $message.Substring(0, 500) }
    @{ ok = $false; error = $message } | ConvertTo-Json -Compress | ForEach-Object { [Console]::Out.WriteLine($_) }
}
