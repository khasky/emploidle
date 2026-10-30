$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

function Pattern($element, $identifier) {
    $value = $null
    if ($element.TryGetCurrentPattern($identifier, [ref]$value)) { return $value }
    return $null
}

try {
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $request = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]([long]$request.window))
    if ($null -eq $root -or $root.Current.ProcessId -ne [int]$request.process_id -or $root.Current.IsOffscreen) {
        throw 'The selected window is unavailable or belongs to a different process'
    }
    $rootRect = $root.Current.BoundingRectangle
    $scope = [System.Windows.Automation.TreeScope]::Descendants
    $visible = [System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::IsOffscreenProperty, $false)
    $elements = $root.FindAll($scope, $visible)
    $domain = $null
    $text = ''
    $rows = [Collections.Generic.List[object]]::new()
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
        if ($kind -eq 'ControlType.Document' -and $text.Length -eq 0) {
            $document = Pattern $element ([System.Windows.Automation.TextPattern]::Pattern)
            if ($null -ne $document) { $text = $document.DocumentRange.GetText(3000) }
        }
        if ($kind -eq 'ControlType.Text' -and $text.Length -lt 3000) {
            $name = [string]$current.Name
            if ($name.Length -gt 0) { $text = ($text + "`n" + $name).Substring(0, [Math]::Min(3000, $text.Length + 1 + $name.Length)) }
        }
        if ($rows.Count -lt 24 -and $kind -match '^ControlType\.(Text|ListItem|DataItem)$') {
            $left = [Math]::Max($rectangle.Left, $rootRect.Left)
            $top = [Math]::Max($rectangle.Top, $rootRect.Top)
            $right = [Math]::Min($rectangle.Right, $rootRect.Right)
            $bottom = [Math]::Min($rectangle.Bottom, $rootRect.Bottom)
            if ($right -gt $left -and $bottom -gt $top) {
                $rows.Add(@{ x = [int]$left; y = [int]$top; width = [int]($right - $left); height = [int]($bottom - $top) })
            }
        }
        if ($kind -eq 'ControlType.Edit' -and $null -eq $domain) {
            $identifier = [string]$current.AutomationId
            $name = [string]$current.Name
            if ($identifier -match '^(addressEditBox|urlbar-input|urlbar|omnibox|location-bar|addressbar|address-field)$' -or
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
    if ($text.Length -eq 0 -and $rows.Count -eq 0 -and $null -eq $domain) {
        throw 'The selected application does not expose supported accessibility content'
    }
    $data = @{ domain = $domain; text = $text; rows = @($rows.ToArray()) }
    @{ ok = $true; data = $data } | ConvertTo-Json -Depth 8 -Compress | ForEach-Object { [Console]::Out.Write($_) }
} catch {
    $message = [string]$_.Exception.Message
    if ($message.Length -gt 500) { $message = $message.Substring(0, 500) }
    @{ ok = $false; error = $message } | ConvertTo-Json -Compress | ForEach-Object { [Console]::Out.Write($_) }
}
