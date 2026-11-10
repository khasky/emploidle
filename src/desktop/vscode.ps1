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
    $expanded = Pattern $element ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    $sidebar = $element.Current.ClassName -match '^action-item\s'
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while (-not $selection.Current.IsSelected -or ($sidebar -and $null -ne $expanded -and
        $expanded.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Expanded)) {
        CheckInput
        if ($timer.ElapsedMilliseconds -ge 700) { throw 'The editor control did not finish selecting' }
        Start-Sleep -Milliseconds 15
    }
}

function EditorTree($root) {
    $trees = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Tree))
    return $trees | Where-Object { -not $_.Current.IsOffscreen -and $_.Current.Name -match '^(Files Explorer|Проводник файлов|Обозреватель файлов)$' } | Select-Object -First 1
}

function EditorSelectionRestorable($selected, $editors) {
    if ($selected.Count -eq 0) { return $true }
    if ($selected.Count -ne 1) { return $false }
    $item = $selected[0]
    $expand = Pattern $item ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
    if ($null -eq $expand) { return $false }
    if ($expand.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::LeafNode) { return $true }
    $matches = @($editors | Where-Object { (EditorFileName $_) -eq $item.Current.Name })
    if ($matches.Count -le 1) { return $matches.Count -eq 1 }
    $path = EditorFilePath $item
    if ($null -eq $path) { return $false }
    foreach ($editor in $matches) {
        if ((EditorFilePath $editor) -cne $path) { return $false }
    }
    return $true
}

function EditorFilePath($element) {
    $condition = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Group)
    $labels = $element.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $paths = @($labels | Where-Object { $_.Current.ClassName -match '(^|\s)monaco-icon-label(\s|$)' -and
        [IO.Path]::IsPathRooted($_.Current.Name) } | ForEach-Object { $_.Current.Name } | Select-Object -Unique)
    if ($paths.Count -eq 1) { return $paths[0] }
    $selection = Pattern $element ([System.Windows.Automation.SelectionItemPattern]::Pattern)
    if ($null -eq $selection -or -not $selection.Current.IsSelected) { return $null }
    $group = $element
    for ($depth = 0; $depth -lt 20 -and $null -ne $group; $depth++) {
        if ($group.Current.ClassName -match '(^|\s)split-view-view(\s|$)') { break }
        $group = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($group)
    }
    if ($null -eq $group) { return $null }
    $labels = $group.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
    $paths = @($labels | Where-Object { $_.Current.ClassName -match '(^|\s)monaco-icon-label(\s|$)' -and
        $_.Current.ClassName -notmatch '(^|\s)tab-label(\s|$)' -and [IO.Path]::IsPathRooted($_.Current.Name) } |
        ForEach-Object { $_.Current.Name } | Select-Object -Unique)
    if ($paths.Count -eq 1) { return $paths[0] }
    return $null
}

function EditorFiles($tree, $editors) {
    $names = @($editors | ForEach-Object { EditorFileName $_ })
    $items = $tree.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TreeItem))
    return @($items | Where-Object {
        -not $_.Current.IsOffscreen -and $_.Current.Name -match '\.(rs|js|mjs|cjs|ts|mts|cts|tsx|jsx|vue|svelte|py|go|java|kt|swift|rb|php|cs|cpp|cc|c|h|md|txt|json|toml|yaml|yml|css|scss|html|xml|sql|sh|ps1)$' -and
        $names -notcontains $_.Current.Name -and $request.exclude_files -notcontains $_.Current.Name -and
        (Pattern $_ ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::LeafNode
    })
}

function PrepareEditorFile($root, $session, [uint32]$choice) {
    $tree = EditorTree $root
    if ($null -eq $tree) { $session.reason = 'Explorer tree is unavailable'; return $false }
    $selected = @((Pattern $tree ([System.Windows.Automation.SelectionPattern]::Pattern)).Current.GetSelection())
    $editors = (EditorControls $root).editors
    $selectedFileOnly = $editors.Count -eq 0 -and $selected.Count -eq 1 -and
        (Pattern $selected[0] ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::LeafNode
    if (-not $selectedFileOnly -and -not (EditorSelectionRestorable $selected $editors)) { $session.reason = 'Explorer selection refers to a closed or ambiguous file'; return $false }
    if ($selected.Count -eq 0 -and $null -eq (EditorTreeBlankPoint $tree)) { $session.reason = 'The empty Explorer selection has no restorable blank area'; return $false }
    $session.tree_selection = @($selected | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
    $session.tree_captured = $true
    $budget = [Diagnostics.Stopwatch]::StartNew()
    for ($depth = 0; -not $selectedFileOnly -and $depth -lt 6 -and $budget.ElapsedMilliseconds -lt 1200; $depth++) {
        CheckInput
        $items = $tree.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TreeItem))
        $folders = @($items | Where-Object { -not $_.Current.IsOffscreen -and
            (Pattern $_ ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed })
        if ($folders.Count -eq 0) { break }
        $folder = $folders[[int](([Math]::Floor($choice / 3) + $depth) % $folders.Count)]
        $id = @($folder.GetRuntimeId())
        $session.expanded_folders += ,$id
        EditorSelect $folder
        $folder.SetFocus()
        CheckInput
        $expansion = Pattern $folder ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
        if ($expansion.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed) { $expansion.Expand() }
        $timer = [Diagnostics.Stopwatch]::StartNew()
        do {
            CheckInput
            $files = @(EditorFiles $tree (EditorControls $root).editors)
            if ($files.Count -gt 0) { break }
            Start-Sleep -Milliseconds 15
        } while ($timer.ElapsedMilliseconds -lt 700 -and $budget.ElapsedMilliseconds -lt 1200)
        if ($files.Count -gt 0) { break }
    }
    $files = @(EditorFiles $tree (EditorControls $root).editors)
    if ($selectedFileOnly) {
        # Keeping this selection avoids reopening a closed file during restoration.
        $files = @($files | Where-Object { SameElement $_ $selected[0].GetRuntimeId() })
    }
    if ($files.Count -eq 0) { $session.reason = 'Explorer exposes no eligible unopened text file'; return $false }
    $file = $files[[int]([Math]::Floor($choice / 3) % $files.Count)]
    $scrollItem = Pattern $file ([System.Windows.Automation.ScrollItemPattern]::Pattern)
    if ($null -ne $scrollItem) { CheckInput; $scrollItem.ScrollIntoView() }
    $bounds = $file.Current.BoundingRectangle
    $point = $null
    foreach ($offset in @(8, ($bounds.Width / 4), ($bounds.Width / 2))) {
        CheckInput
        $candidate = [System.Windows.Point]::new($bounds.Left + $offset, $bounds.Top + $bounds.Height / 2)
        $hit = EditorHitElement $candidate
        for ($depth = 0; $depth -lt 12 -and $null -ne $hit; $depth++) {
            if (SameElement $hit $file.GetRuntimeId()) { $point = $candidate; break }
            $hit = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($hit)
        }
        if ($null -ne $point) { break }
    }
    if ($null -eq $point) { $session.reason = 'The project file has no confirmed clickable point'; return $false }
    CheckInput
    $session.file = @{ element = @($file.GetRuntimeId()); name = $file.Current.Name; bounds = @{ x = [int]$point.X - 1; y = [int]$point.Y - 1; width = 2; height = 2 } }
    return $true
}

function EditorInputOwned {
    return [EmploidleDesktop]::InputTick() -eq [uint32]$request.input_tick -and [EmploidleDesktop]::GetForegroundWindow().ToInt64() -eq [long]$request.window
}

function RestoreEditorTreeSelection($tree, $ids) {
    if ($ids.Count -eq 0) {
        $selection = Pattern $tree ([System.Windows.Automation.SelectionPattern]::Pattern)
        if (@($selection.Current.GetSelection()).Count -gt 0) {
            $point = EditorTreeBlankPoint $tree
            if ($null -eq $point) { throw 'The empty Explorer selection cannot be restored' }
            CheckInput
            CheckEditorModifiers
            [EmploidleDesktop]::ClickClient([IntPtr]([long]$request.window), $point.x, $point.y, [uint32]$request.input_tick)
            $timer = [Diagnostics.Stopwatch]::StartNew()
            while (@($selection.Current.GetSelection()).Count -ne 0) {
                CheckInput
                if ($timer.ElapsedMilliseconds -ge 700) { throw 'The empty Explorer selection did not restore' }
                Start-Sleep -Milliseconds 15
            }
        }
    } else {
        foreach ($id in $ids) {
            $element = EditorElement $tree $id
            CheckInput
            if ($null -ne $element) { EditorSelect $element }
        }
    }
}

function EditorTreeBlankPoint($tree) {
    $bounds = $tree.Current.BoundingRectangle
    if ($bounds.Width -lt 20 -or $bounds.Height -lt 20) { return $null }
    $point = [System.Windows.Point]::new($bounds.Left + $bounds.Width / 2, $bounds.Bottom - 8)
    $items = $tree.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TreeItem))
    foreach ($item in $items) {
        if (-not $item.Current.IsOffscreen -and $item.Current.BoundingRectangle.Contains($point)) { return $null }
    }
    $hit = EditorHitElement $point
    for ($depth = 0; $depth -lt 20 -and $null -ne $hit; $depth++) {
        if ($hit.Current.ControlType -eq [System.Windows.Automation.ControlType]::TreeItem) { return $null }
        if (SameElement $hit $tree.GetRuntimeId()) { return @{ x = [int]$point.X; y = [int]$point.Y } }
        $hit = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($hit)
    }
    return $null
}

function ReviewEditorTree($root, [uint32]$index) {
    $tree = EditorTree $root
    if ($null -eq $tree) { Respond $false; return }
    $selected = @((Pattern $tree ([System.Windows.Automation.SelectionPattern]::Pattern)).Current.GetSelection())
    $controls = EditorControls $root
    if ($null -eq $controls -or -not (EditorSelectionRestorable $selected $controls.editors)) { Respond $false; return }
    if ($selected.Count -eq 0 -and $null -eq (EditorTreeBlankPoint $tree)) { Respond $false; return }
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
        if ($pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed) { $pattern.Expand() }
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
            RestoreEditorTreeSelection $tree @($selected | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
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
        for ($index = $session.expanded_folders.Count - 1; $index -ge 0; $index--) {
            $folder = EditorElement $root $session.expanded_folders[$index]
            if ($null -ne $folder) {
                CheckInput
                EditorSelect $folder
                $folder.SetFocus()
                CheckInput
                $pattern = Pattern $folder ([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
                if ($null -ne $pattern -and $pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded) {
                    $pattern.Collapse()
                    $timer = [Diagnostics.Stopwatch]::StartNew()
                    while ($pattern.Current.ExpandCollapseState -ne [System.Windows.Automation.ExpandCollapseState]::Collapsed) {
                        CheckInput
                        if ($timer.ElapsedMilliseconds -ge 700) { throw 'The project folder did not finish restoring' }
                        Start-Sleep -Milliseconds 15
                    }
                }
            }
        }
        if ($session.tree_captured) {
            $tree = EditorTree $root
            if ($null -ne $tree) {
                RestoreEditorTreeSelection $tree $session.tree_selection
            }
        }
        $original = EditorElement $root $session.original_view
        CheckInput
        if ($null -ne $original) { EditorSelect $original }
    }
    if (-not $interrupted -and (EditorInputOwned)) {
        if ($restoreEditor) {
            foreach ($id in $session.selected_editors) {
                $editor = EditorElement $root $id
                CheckInput
                if ($null -ne $editor) { EditorSelect $editor }
            }
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

    }
    if ($action -eq 2 -and $controls.editors.Count -gt 0 -and $null -eq $activeEditor) { Respond @{ session = $null }; return }
    $focus = [System.Windows.Automation.AutomationElement]::FocusedElement
    $session = @{
        original_view = @($controls.original.GetRuntimeId()); target_view = @($target.GetRuntimeId())
        original_focus = if ($null -ne $focus -and $focus.Current.ProcessId -eq $root.Current.ProcessId) { @($focus.GetRuntimeId()) } else { $null }
        created_editor = $null; tree_selection = @(); tree_captured = $false; expanded_folders = @(); file = $null
        original_editors = @($controls.editors | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
        selected_editors = @($controls.editors | Where-Object { (Pattern $_ ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected } | ForEach-Object { ,([int[]]$_.GetRuntimeId()) })
    }
    $committed = $false
    try {
        CheckInput
        EditorSelect $target
        $timer = [Diagnostics.Stopwatch]::StartNew()
        while (-not (Pattern $target ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected -and $timer.ElapsedMilliseconds -lt 700) { CheckInput; Start-Sleep -Milliseconds 15 }
        if (-not (Pattern $target ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected) { throw 'The editor sidebar did not open' }
        if ($action -eq 2) {
            if (-not (PrepareEditorFile $root $session $choice)) { Respond @{ session = $null; reason = $session.reason }; return }
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

function EditorFileName($tab) {
    return $tab.Current.Name -replace ', (Editor Group|Группа редакторов) \d+$', '' -replace ', (preview|предпросмотр|предварительный просмотр)$', ''
}

function CheckEditorModifiers {
    foreach ($key in @(0x10, 0x11, 0x12, 0x5b, 0x5c)) {
        if ([EmploidleDesktop]::GetAsyncKeyState($key) -lt 0) { throw 'The project file action was interrupted by a held modifier' }
    }
}

function EditorHitElement($point) {
    $cache = [System.Windows.Automation.CacheRequest]::new()
    $cache.TreeFilter = [System.Windows.Automation.Automation]::RawViewCondition
    $lease = $cache.Activate()
    try { return [System.Windows.Automation.AutomationElement]::FromPoint($point) } finally { $lease.Dispose() }
}

function EditorFileTarget($root, $session) {
    CheckInput
    CheckEditorModifiers
    $file = EditorElement $root $session.file.element
    if ($null -eq $file -or $file.Current.Name -cne $session.file.name -or $file.Current.IsOffscreen -or
        $file.Current.ControlType -ne [System.Windows.Automation.ControlType]::TreeItem) { throw 'The project file moved before opening' }
    $bounds = $file.Current.BoundingRectangle
    $target = $session.file.bounds
    $point = [System.Windows.Point]::new($target.x + $target.width / 2, $target.y + $target.height / 2)
    if (-not $bounds.Contains($point)) { throw 'The project file moved before opening' }
    $element = EditorHitElement $point
    $hitType = if ($null -ne $element) { $element.Current.ControlType.ProgrammaticName } else { 'unavailable' }
    $found = $false
    for ($depth = 0; $depth -lt 12 -and $null -ne $element; $depth++) {
        if (SameElement $element $session.file.element) { $found = $true; break }
        $element = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($element)
    }
    if (-not $found) { throw "The project file is covered by another control ($hitType)" }
    CheckInput
    return $target
}

function CaptureEditor($root, $session) {
    CheckInput
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        $controls = EditorControls $root
        $added = @($controls.editors | Where-Object { $candidate = $_; -not @($session.original_editors | Where-Object { SameElement $candidate $_ }).Count })
        CheckInput
        if ($added.Count -eq 1 -and (EditorFileName $added[0]) -ceq $session.file.name) { break }
        Start-Sleep -Milliseconds 15
    } while ($timer.ElapsedMilliseconds -lt 700)
    if ($added.Count -ne 1 -or (EditorFileName $added[0]) -cne $session.file.name) { throw 'The opened project editor could not be identified; preserving it' }
    foreach ($id in $session.original_editors) {
        if (-not @($controls.editors | Where-Object { SameElement $_ $id }).Count) { throw 'A preexisting editor changed; preserving the current editors' }
    }
    CheckInput
    $session.created_editor = @($added[0].GetRuntimeId())
    $session.file = $null
    $null = EditorClose $added[0]
    $committed = $false
    try {
        Respond $session $true
        $committed = [Console]::In.ReadLine() -eq 'commit'
    } finally {
        if (-not $committed) { RestoreEditor $root $session ([EmploidleDesktop]::InputTick() -ne [uint32]$request.input_tick) }
    }
}

function EditorViewport($root, $session) {
    CheckInput
    CheckEditorModifiers
    $created = EditorElement $root $session.created_editor
    if ($null -eq $created) { throw 'The owned project file is no longer available' }
    if (-not (Pattern $created ([System.Windows.Automation.SelectionItemPattern]::Pattern)).Current.IsSelected) {
        throw 'The owned project file is no longer active; preserving the current view'
    }
    $null = EditorClose $created
    $edits = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit))
    $matches = @($edits | Where-Object { $_.Current.ClassName -match '(^|\s)(native-edit-context|inputarea)(\s|$)' -and (EditorFileName $_) -ceq (EditorFileName $created) })
    if ($matches.Count -ne 1) { throw 'The owned project editor viewport is ambiguous' }
    $viewport = $matches[0]
    for ($depth = 0; $depth -lt 20 -and $null -ne $viewport; $depth++) {
        if ($viewport.Current.ClassName -match '(^|\s)monaco-editor(\s|$)') { break }
        $viewport = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($viewport)
    }
    if ($null -eq $viewport -or $viewport.Current.ClassName -notmatch '(^|\s)monaco-editor(\s|$)' -or $viewport.Current.IsOffscreen) {
        throw 'The owned project editor viewport is unavailable'
    }
    $bounds = $viewport.Current.BoundingRectangle
    if ($bounds.Width -lt 120 -or $bounds.Height -lt 120) { throw 'The owned project editor viewport is too small' }
    $hit = EditorHitElement ([System.Windows.Point]::new($bounds.Left + $bounds.Width / 2, $bounds.Top + $bounds.Height / 2))
    $found = $false
    for ($depth = 0; $depth -lt 20 -and $null -ne $hit; $depth++) {
        if (SameElement $hit $viewport.GetRuntimeId()) { $found = $true; break }
        $hit = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($hit)
    }
    if (-not $found) { throw 'The owned project editor viewport is covered by another control' }
    CheckInput
    return @{ x = [int]$bounds.Left; y = [int]$bounds.Top; width = [int]$bounds.Width; height = [int]$bounds.Height }
}
