# asis を Windows から消す。設定(%APPDATA%\asis)は残す。
$dir = Join-Path $env:LOCALAPPDATA 'Programs\asis'
Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue
Remove-Item -Force (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\AsIs.lnk') -ErrorAction SilentlyContinue
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
[Environment]::SetEnvironmentVariable('Path', (($path -split ';' | Where-Object { $_ -and $_ -ne $dir }) -join ';'), 'User')
Write-Host 'AsIs を消しました。設定は %APPDATA%\asis に残っています。'
