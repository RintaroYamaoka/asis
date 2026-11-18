# asis を Windows に入れる。このファイルと同じフォルダの asis-gui.exe・asis.exe を写して、スタートメニューに置く。
# 使い方: このフォルダを右クリック →「ターミナルで開く」→  powershell -ExecutionPolicy Bypass -File install.ps1
# 管理者の権限は要らない(自分のユーザーの中にだけ入れる)。消すときは uninstall.ps1。
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\asis'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
foreach ($f in 'asis-gui.exe', 'asis.exe') {
    Copy-Item -Force (Join-Path $PSScriptRoot $f) $dir
}

# スタートメニューに「asis」を置く
$menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\asis.lnk'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($menu)
$link.TargetPath = Join-Path $dir 'asis-gui.exe'
$link.WorkingDirectory = $env:USERPROFILE
$link.Save()

# コマンド `asis` を打てるように、自分のユーザーの PATH に足す
$path = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($path -split ';') -notcontains $dir) {
    [Environment]::SetEnvironmentVariable('Path', ($path.TrimEnd(';') + ';' + $dir), 'User')
}

Write-Host "asis を入れました: $dir"
Write-Host 'スタートメニューの「asis」から開けます。'
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Write-Host ''
    Write-Host 'GitHub の公式コマンド gh が見つかりません。asis は gh のログインを借りて読みます。'
    Write-Host '  winget install GitHub.cli   のあと   gh auth login'
}
