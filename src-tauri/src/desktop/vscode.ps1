function EditorControls($root) {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem)
    $items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $explorer = $null
    $extensions = $null
    $editors = [Collections.Generic.List[object]]::new()
    foreach ($item in $items) {
        if ($item.Current.ClassName -match '^tab(\s|$)') { $editors.Add($item); continue }
        if ($item.Current.ClassName -notmatch '^action-item\s') { continue }
        if ($item.Current.Name -match '^(Explorer|Проводник)(\s*\(|$)') { $explorer = $item }
        if ($item.Current.Name -match '^(Extensions|Расширения)(\s*\(|$)') { $extensions = $item }
    }
    if ($null -eq $explorer -or $null -eq $extensions) { return $null }
    $container = (Pattern $explorer ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.SelectionContainer
    $original = $null
    foreach ($item in $items) {
        $selection = Pattern $item ([System.Windows.Automation.SelectionItemPattern]::Pattern)
        if ($null -ne $selection -and $selection.Current.IsSelected -and
            [System.Windows.Automation.Automation]::Compare($selection.Current.SelectionContainer, $container)) { $original = $item; break }
    }
    if ($null -eq $original) { return $null }
    $expanded = Pattern $original ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    if ($null -eq $expanded -or $expanded.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Expanded) { return $null }
    return @{ explorer = $explorer; extensions = $extensions; original = $original; editors = @($editors.ToArray()) }
}

function EditorElement($root, $runtimeId) {
    if ($null -eq $runtimeId) { return $null }
    $elements = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $elements) { if (SameElement $element $runtimeId) { return $element } }
    return $null
}

function EditorSelect($element) {
    $selection = Pattern $element ([System.Windows.Automation.SelectionItemPattern]::Pattern)
    if ($null -eq $selection) { throw 'The editor control is no longer selectable' }
    if (-not $selection.Current.IsSelected) { $selection.Select() }
}

function EditorTree($root) {
    $trees = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Tree))
    return $trees | Where-Object { -not $_.Current.IsOffscreen -and $_.Current.Name -match '^(Files Explorer|Проводник файлов|Обозреватель файлов)$' } | Select-Object -First 1
}

function EditorSelectionRestorable($selected, $editors) {
    if ($selected.Count -ne 1) { return $false }
    $item = $selected[0]
    $expand = Pattern $item ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    return $null -ne $expand -and ($expand.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::LeafNode -or @($editors | Where-Object { $_.Current.Name -eq $item.Current.Name }).Count -gt 0)
}

function EditorInputOwned {
    return [EmploidleDesktop]::InputTick() -eq [uint32]$request.input_tick -and [EmploidleDesktop]::GetForegroundWindow().ToInt64() -eq [long]$request.window
}

function ReviewEditorTree($root, [uint32]$index) {
    $tree = EditorTree $root
    if ($null -eq $tree) { Respond $false; return }
    $selected = @((Pattern $tree ([System.Windows.Automation.SelectionPattern]::Pattern)).Current.GetSelection())
    $controls = EditorControls $root
    if ($null -eq $controls -or -not (EditorSelectionRestorable $selected $controls.editors)) { Respond $false; return }
    $focus = [System.Windows.Automation.AutomationElement]::FocusedElement
    $items = $tree.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TreeItem))
    $folders = @($items | Where-Object { -not $_.Current.IsOffscreen -and (Pattern $_ ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed })
    if ($folders.Count -eq 0) { Respond $false; return }
    $folder = $folders[[int]($index % $folders.Count)]
    $pattern = Pattern $folder ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    CheckInput
    try {
        EditorSelect $folder
        $folder.SetFocus()
        CheckInput
        $pattern.Expand()
        $timer = [Diagnostics.Stopwatch]::StartNew()
        while ($pattern.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Expanded -and $timer.ElapsedMilliseconds -lt 700) { CheckInput; Start-Sleep -Milliseconds 15 }
        if ($pattern.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Expanded) { throw 'The project folder did not expand' }
        $timer.Restart()
        while ($timer.ElapsedMilliseconds -lt 450) { CheckInput; Start-Sleep -Milliseconds 15 }
    } finally {
        if ($pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded) { $pattern.Collapse() }
        $timer = [Diagnostics.Stopwatch]::StartNew()
        while ($pattern.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Collapsed -and $timer.ElapsedMilliseconds -lt 700) { Start-Sleep -Milliseconds 15 }
        if ($pattern.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Collapsed) { throw 'The project folder did not return to its collapsed state' }
        if (EditorInputOwned) {
            CheckInput
            EditorSelect $selected[0]
            CheckInput
            if ($null -ne $focus -and -not $focus.Current.IsOffscreen) { $focus.SetFocus() }
        }
    }
    Respond $true
}

function EditorClose($tab) {
    if ($tab.Current.ClassName -match '(^|\s)dirty(\s|$)' -or $tab.Current.Name -match 'unsaved|несохран') {
        throw 'The temporary editor has user edits; preserving it'
    }
    $buttons = $tab.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
    foreach ($button in $buttons) {
        if ($button.Current.ClassName -match '(^|\s)codicon-close(-small)?(\s|$)' -and $button.Current.Name -match '^(Close|Закрыть)(\s*\(|$)') {
            $invoke = Pattern $button ([System.Windows.Automation.InvokePattern]::Pattern)
            if ($null -ne $invoke) { return $invoke }
        }
    }
    throw 'The temporary editor has no safe close control; preserving it'
}

function RestoreEditor($root, $session, [bool]$interrupted) {
    $restoreEditor = $false
    if ($null -ne $session.created_editor) {
        $created = EditorElement $root $session.created_editor
        if ($null -ne $created) {
            $selection = Pattern $created ([System.Windows.Automation.SelectionItemPattern]::Pattern)
            $restoreEditor = $selection.Current.IsSelected
            $close = EditorClose $created
            $close.Invoke()
            $timer = [Diagnostics.Stopwatch]::StartNew()
            while ($null -ne (EditorElement $root $session.created_editor) -and $timer.ElapsedMilliseconds -lt 700) { Start-Sleep -Milliseconds 15 }
            if ($null -ne (EditorElement $root $session.created_editor)) { throw 'The temporary editor did not close' }
        }
    }
    $target = EditorElement $root $session.target_view
    $targetSelection = if ($null -ne $target) { Pattern $target ([System.Windows.Automation.SelectionItemPattern]::Pattern) } else { $null }
    if (-not $interrupted -and (EditorInputOwned) -and $null -ne $targetSelection -and $targetSelection.Current.IsSelected) {
        CheckInput
        if ($session.tree_selection.Count -gt 0) {
            foreach ($id in $session.tree_selection) {
                $element = EditorElement $root $id
                CheckInput
                if ($null -ne $element) { EditorSelect $element }
            }
        }
        $original = EditorElement $root $session.original_view
        CheckInput
        if ($null -ne $original) { EditorSelect $original }
    }
    if (-not $interrupted -and (EditorInputOwned)) {
        if ($restoreEditor -and $null -ne $session.original_editor) {
            $editor = EditorElement $root $session.original_editor
            CheckInput
            if ($null -ne $editor) { EditorSelect $editor }
        }
        $focus = EditorElement $root $session.original_focus
        CheckInput
        if ($null -ne $focus -and -not $focus.Current.IsOffscreen) { $focus.SetFocus() }
    }
}

function BeginEditor($root, [uint32]$choice) {
    $action = [int]($choice % 3)
    $controls = EditorControls $root
    if ($null -eq $controls) { Respond @{ session = $null }; return }
    $target = if ($action -eq 0) { $controls.extensions } else { $controls.explorer }
    if ($action -eq 2 -and -not (SameElement $controls.original $controls.explorer.GetRuntimeId())) { Respond @{ session = $null }; return }
    if ($action -ne 2 -and (SameElement $target $controls.original.GetRuntimeId())) { Respond @{ session = $null }; return }
    $activeEditor = $null
    foreach ($editor in $controls.editors) {
        if ((Pattern $editor ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected) { $activeEditor = $editor }
        if ($action -eq 2) {
            # A file selected in Explorer replaces an existing preview, so refuse that layout.
            $children = $editor.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
            if ($editor.Current.Name -match ',|unsaved|несохран' -or @($children | Where-Object { $_.Current.ClassName -match '(^|\s)italic(\s|$)' }).Count -gt 0) {
                Respond @{ session = $null }; return
            }
        }
    }
    if ($action -eq 2) {
        $containers = @($controls.editors | ForEach-Object { ((Pattern $_ ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.SelectionContainer.GetRuntimeId()) -join ',' } | Select-Object -Unique)
        if ($containers.Count -ne 1 -or $null -eq $activeEditor) { Respond @{ session = $null }; return }
    }
    $focus = [System.Windows.Automation.AutomationElement]::FocusedElement
    $session = @{
        original_view = @($controls.original.GetRuntimeId()); target_view = @($target.GetRuntimeId())
        original_focus = if ($null -ne $focus -and $focus.Current.ProcessId -eq $root.Current.ProcessId) { @($focus.GetRuntimeId()) } else { $null }
        original_editor = if ($null -ne $activeEditor) { @($activeEditor.GetRuntimeId()) } else { $null }
        created_editor = $null; tree_selection = @()
    }
    $committed = $false
    try {
        CheckInput
        EditorSelect $target
        $timer = [Diagnostics.Stopwatch]::StartNew()
        while (-not (Pattern $target ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected -and $timer.ElapsedMilliseconds -lt 700) { CheckInput; Start-Sleep -Milliseconds 15 }
        if (-not (Pattern $target ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected) { throw 'The editor sidebar did not open' }
        if ($action -eq 2) {
            $tree = EditorTree $root
            if ($null -eq $tree) { Respond @{ session = $null }; return }
            $selection = Pattern $tree ([System.Windows.Automation.SelectionPattern]::Pattern)
            $selected = @($selection.Current.GetSelection())
            $items = $tree.FindAll([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TreeItem))
            $names = @($controls.editors | ForEach-Object { $_.Current.Name })
            $files = @($items | Where-Object {
                -not $_.Current.IsOffscreen -and $_.Current.Name -match '\.(rs|js|ts|tsx|jsx|py|go|java|cs|cpp|cc|c|h|md|txt|json|toml|yaml|yml|css|html)$' -and
                $names -notcontains $_.Current.Name -and
                (Pattern $_ ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::LeafNode
            })
            if ($files.Count -eq 0) { Respond @{ session = $null }; return }
            if (-not (EditorSelectionRestorable $selected $controls.editors)) { Respond @{ session = $null }; return }
            $session.tree_selection = @($selected | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
            CheckInput
            $file = $files[[int]([Math]::Floor($choice / 3) % $files.Count)]
            EditorSelect $file
            $file.SetFocus()
            $timer = [Diagnostics.Stopwatch]::StartNew()
            do {
                $fresh = EditorControls $root
                $added = @($fresh.editors | Where-Object { $candidate = $_; -not @($controls.editors | Where-Object { SameElement $candidate $_.GetRuntimeId() }).Count })
                CheckInput
                if ($added.Count -eq 1) { $session.created_editor = @($added[0].GetRuntimeId()); break }
                Start-Sleep -Milliseconds 15
            } while ($timer.ElapsedMilliseconds -lt 700)
            if ($null -eq $session.created_editor) { throw 'The opened editor could not be identified' }
            $null = EditorClose $added[0]
        }
        Respond @{ session = $session } $true
        $committed = [Console]::In.ReadLine() -eq 'commit'
    } finally {
        if (-not $committed) {
            $interrupted = [EmploidleDesktop]::InputTick() -ne [uint32]$request.input_tick
            RestoreEditor $root $session $interrupted
        }
    }
    if (-not $committed) { return }
}
