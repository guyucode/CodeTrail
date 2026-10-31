$ErrorActionPreference = 'Stop'
$out = Join-Path $PWD 'release-artifacts/windows'
New-Item -ItemType Directory -Force $out | Out-Null
$installers = @(Get-ChildItem 'src-tauri/target/release/bundle/nsis/*-setup.exe')
if ($installers.Count -ne 1) { throw 'Expected exactly one NSIS installer' }
Copy-Item $installers[0].FullName (Join-Path $out 'CodeTrail_0.2.0_windows_x64_setup.exe')
$tools = Join-Path $env:RUNNER_TEMP 'CodeTrail-tools-0.2.0-windows-x64'
New-Item -ItemType Directory -Force $tools | Out-Null
Copy-Item 'target/release/reader-cli.exe' $tools
Copy-Item 'sdk','docs','third-party-licenses','third-party-sources' $tools -Recurse
Copy-Item 'LICENSE','COMMERCIAL-LICENSE.md','THIRD_PARTY_NOTICES.md','README.md' $tools
Compress-Archive -Path $tools -DestinationPath (Join-Path $out 'CodeTrail-tools-0.2.0-windows-x64.zip') -CompressionLevel Optimal
$lines = Get-ChildItem $out -File | Sort-Object Name | ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $($_.Name)"
}
$lines | Set-Content (Join-Path $out 'SHA256SUMS-windows.txt') -Encoding utf8NoBOM
