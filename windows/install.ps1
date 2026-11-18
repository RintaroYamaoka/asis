# asis を Windows に入れる。このファイルと同じフォルダの exe を写して、スタートメニューに「asis」を置く。
# 使い方: このフォルダを右クリック →「ターミナルで開く」→  powershell -ExecutionPolicy Bypass -File install.ps1
# 管理者の権限は要らない(自分のユーザーの中にだけ入れる)。消すときは uninstall.ps1。
#   -Exe  スタートメニューから開く exe(既定は asis-gui.exe)
#   -Wsl  仕事場が WSL の中のとき、その distro の名前(例: Ubuntu)。窓は Windows 版、gh や設定は WSL の中のものを使う
param([string]$Exe = 'asis-gui.exe', [string]$Wsl = '')
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\asis'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
Copy-Item -Force (Join-Path $PSScriptRoot '*.exe') $dir

# スタートメニューに「asis」を置く
$menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\asis.lnk'
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($menu)
$link.TargetPath = Join-Path $dir $Exe
$link.Arguments = if ($Wsl) { "--wsl $Wsl" } else { '' }
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
