$ErrorActionPreference = 'Stop'
$version = (Get-Content 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json).version
$installer = Join-Path $PWD "release-artifacts/windows/CodeTrail_${version}_windows_x64_setup.exe"
$installRoot = Join-Path $env:RUNNER_TEMP 'CodeTrailInstalledQA'
$installed = Start-Process -FilePath $installer -ArgumentList "/S /D=$installRoot" -Wait -PassThru
if ($installed.ExitCode -ne 0) { throw "Installer failed: $($installed.ExitCode)" }
$app = Join-Path $installRoot 'codetrail-desktop.exe'
if (-not (Test-Path $app)) { throw "Application missing at $app" }
$license = Get-ChildItem $installRoot -Recurse -File -Filter LICENSE | Where-Object { $_.FullName -match '[\\/]legal[\\/]LICENSE$' }
if (-not $license) { throw 'Installed license resource missing' }
$notice = Get-ChildItem $installRoot -Recurse -File -Filter THIRD_PARTY_NOTICES.md
if (-not $notice) { throw 'Installed third-party notices missing' }
$script = Get-ChildItem $installRoot -Recurse -File -Filter langserver.index.js
if (-not $script) { throw 'Installed Pyright resource missing' }
$builtBytes = [System.IO.File]::ReadAllBytes((Join-Path $PWD 'src-tauri/target/release/codetrail-desktop.exe'))
# Tauri patches exactly this bundle-type token before NSIS packaging, then
# restores the original build file. Compare every byte against that expected
# NSIS variant, rather than accepting arbitrary installer changes.
# https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs
$token = [Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_UNK')
$replacement = [Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_NSS')
$tokenOffset = [Text.Encoding]::ASCII.GetString($builtBytes).IndexOf('__TAURI_BUNDLE_TYPE_VAR_UNK',[StringComparison]::Ordinal)
if ($tokenOffset -lt 0) { throw 'Expected Tauri bundle-type token missing' }
[Array]::Copy($replacement,0,$builtBytes,$tokenOffset,$token.Length)
$sha = [Security.Cryptography.SHA256]::Create()
try { $expectedHash = [BitConverter]::ToString($sha.ComputeHash($builtBytes)).Replace('-','') } finally { $sha.Dispose() }
if ((Get-FileHash $app).Hash -ne $expectedHash) { throw 'Installed executable differs from expected NSIS build variant' }
$bytes = [System.IO.File]::ReadAllBytes($app)
$peOffset = [BitConverter]::ToInt32($bytes,0x3c)
if ([BitConverter]::ToUInt16($bytes,$peOffset+4) -ne 0x8664) { throw 'Expected Windows x64 executable' }
# Ask WebView2 to expose its accessibility tree for this isolated smoke test.
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--force-renderer-accessibility'
$process = Start-Process -FilePath $app -PassThru
try {
    $deadline = (Get-Date).AddSeconds(60)
    do {
        Start-Sleep -Milliseconds 500
        $process.Refresh()
        if ($process.HasExited) { throw "App exited before showing a window: $($process.ExitCode)" }
    } while ($process.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline)
    if ($process.MainWindowHandle -eq 0) { throw 'No application window appeared' }
    Add-Type -AssemblyName UIAutomationClient
    Add-Type -AssemblyName UIAutomationTypes
    $window = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
    $names = @()
    do {
        $elements = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
        $names = @($elements | ForEach-Object { $_.Current.Name } | Where-Object { $_ })
        $rendered = ($names -join "`n")
        if ($rendered.Contains('选功能') -and $rendered.Contains('模型路由')) { break }
        Start-Sleep -Seconds 1
    } while ((Get-Date) -lt $deadline)
    if (-not ($rendered.Contains('选功能') -and $rendered.Contains('模型路由'))) { throw 'Frontend content not found in native accessibility tree' }
    Add-Type -AssemblyName System.Drawing
    $rect = $window.Current.BoundingRectangle
    $bitmap = New-Object System.Drawing.Bitmap ([int]$rect.Width),([int]$rect.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen([int]$rect.X,[int]$rect.Y,0,0,$bitmap.Size)
        $bitmap.Save((Join-Path $env:RUNNER_TEMP 'codetrail-windows-native.png'))
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
    @{
        ok = $true
        os = [Environment]::OSVersion.VersionString
        architecture = 'x64'
        installerExitCode = $installed.ExitCode
        installedBinaryMatchesBuild = $true
        comparison = 'Exact SHA-256 match after the single documented UNK-to-NSS Tauri bundle token patch.'
        installedBinarySha256 = $expectedHash.ToLowerInvariant()
        licenseResourcesPresent = $true
        windowTitle = $process.MainWindowTitle
        renderedControls = $names
        scope = 'Windows hosted runner: silent installation, native launch and demo frontend accessibility. Physical Windows 10/11 user workflows not manually tested.'
    } | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $env:RUNNER_TEMP 'codetrail-windows-native.json') -Encoding utf8
} finally {
    if (-not $process.HasExited) {
        $null = $process.CloseMainWindow()
        if (-not $process.WaitForExit(10000)) { $process.Kill() }
    }
}
