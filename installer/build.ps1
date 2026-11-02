# Pro Internet Manager Setup - build script (Windows, .NET Framework 4 csc.exe)
# Folders (environment variables, all optional):
#   PIM_PAYLOAD_DIR  folder with ffmpeg.exe, yt-dlp.exe, deno.exe, FFMPEG-NOTICE.txt, YT-DLP-NOTICE.txt
#                    (default: payload-src next to this script; these tools are not in the repository)
#   PIM_APP_EXE      the built app (default: ..\app\Pro Internet Manager_restyle.exe)
#   PIM_CHROMIUM_DIR Chromium extension folder (default: ..\extensions\chromium)
# Optional background picture: assets\background.jpg (not included; without it Setup uses a plain background).
$ErrorActionPreference = 'Stop'
$S   = $PSScriptRoot
$R   = if ($env:PIM_PAYLOAD_DIR) { $env:PIM_PAYLOAD_DIR } else { Join-Path $S 'payload-src' }
$AppExe = if ($env:PIM_APP_EXE) { $env:PIM_APP_EXE } else { Join-Path $S '..\app\Pro Internet Manager_restyle.exe' }
$Chromium = if ($env:PIM_CHROMIUM_DIR) { $env:PIM_CHROMIUM_DIR } else { Join-Path $S '..\extensions\chromium' }
$csc = 'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe'
Set-Location $S
New-Item -ItemType Directory -Force "$S\assets", "$S\_work", "$S\_preview" | Out-Null

# payload (only what the app needs)
if (-not $env:PIM_SKIP_PAYLOAD) {
  if (Test-Path "$S\payload") { Remove-Item "$S\payload" -Recurse -Force }
  New-Item -ItemType Directory -Force "$S\payload" | Out-Null
  Copy-Item $AppExe "$S\payload\Pro Internet Manager.exe"
  foreach ($f in 'ffmpeg.exe','yt-dlp.exe','deno.exe','FFMPEG-NOTICE.txt','YT-DLP-NOTICE.txt') { Copy-Item "$R\$f" "$S\payload\$f" }
  Copy-Item "$S\assets\ProInternetManager.ico" "$S\payload\ProInternetManager.ico"
  Copy-Item $Chromium "$S\payload\chromium-extension" -Recurse
  # English instructions (INSTRUCTIES.txt -> INSTRUCTIONS.txt)
  Remove-Item "$S\payload\chromium-extension\INSTRUCTIES.txt" -Force -ErrorAction SilentlyContinue
  Copy-Item "$S\assets\INSTRUCTIONS.txt" "$S\payload\chromium-extension\INSTRUCTIONS.txt" -Force
  if (Test-Path "$S\_work\payload.zip") { Remove-Item "$S\_work\payload.zip" -Force }
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  [IO.Compression.ZipFile]::CreateFromDirectory("$S\payload", "$S\_work\payload.zip", [IO.Compression.CompressionLevel]::Optimal, $false)
}
"payload.zip: " + [math]::Round((Get-Item "$S\_work\payload.zip").Length / 1MB, 1) + " MB"

$refs = '/r:System.dll', '/r:System.Drawing.dll', '/r:System.Windows.Forms.dll', '/r:System.IO.Compression.dll', '/r:System.IO.Compression.FileSystem.dll'
$common = @('/nologo', '/target:winexe', '/platform:anycpu', '/optimize+', "/win32icon:$S\assets\ProInternetManager.ico", "/win32manifest:$S\app.manifest",
  "/resource:$S\assets\ProInternetManager.ico,ProInternetManager.ico") + $refs
if (Test-Path "$S\assets\background.jpg") { $common += "/resource:$S\assets\background.jpg,bg_upscaler.jpg" } else { 'No assets\background.jpg: Setup uses a plain background.' }

& $csc @common "/out:$S\_work\Uninstall.exe" "$S\InstallerPIM.cs"
if ($LASTEXITCODE -ne 0) { throw 'Uninstall build failed' }
& $csc @common "/resource:$S\_work\payload.zip,payload.zip" "/resource:$S\_work\Uninstall.exe,Uninstall.exe" "/out:$S\Pro Internet Manager Setup.exe" "$S\InstallerPIM.cs"
if ($LASTEXITCODE -ne 0) { throw 'Setup build failed' }
& $csc @common '/define:PREVIEW' "/out:$S\_work\SetupPreview.exe" "$S\InstallerPIM.cs"
if ($LASTEXITCODE -ne 0) { throw 'Preview build failed' }
Get-Item "$S\Pro Internet Manager Setup.exe", "$S\_work\Uninstall.exe" | Select-Object Name, Length | Format-Table -AutoSize
