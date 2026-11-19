# asis を Windows に入れる。このファイルと同じフォルダの exe を写して、スタートメニューに「asis」を置く。
# 使い方: このフォルダを右クリック →「ターミナルで開く」→  powershell -ExecutionPolicy Bypass -File install.ps1
# 管理者の権限は要らない(自分のユーザーの中にだけ入れる)。消すときは uninstall.ps1。
#   -Exe  スタートメニューから開く exe(既定は asis-gui.exe)
#   -Wsl  仕事場が WSL の中のとき、その distro の名前(例: Ubuntu)。窓は Windows 版、gh や設定は WSL の中のものを使う
param([string]$Exe = 'asis-gui.exe', [string]$Wsl = '')
$ErrorActionPreference = 'Stop'
$dir = Join-Path $env:LOCALAPPDATA 'Programs\asis'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
# 動いている exe は上書きできないので、別名に退かしてから新しいものを置く(AsIs を開いたままでも入る)。
# 退かしたものは、次に AsIs を開いたときに消える
# (変数の名前を $exe にしない。PowerShell は大文字と小文字を分けないので、引数の $Exe を書き換えてしまう)
foreach ($file in Get-ChildItem (Join-Path $PSScriptRoot '*.exe')) {
    $to = Join-Path $dir $file.Name
    if (Test-Path $to) {
        Remove-Item -Force "$to.old" -ErrorAction SilentlyContinue
        Rename-Item $to "$($file.Name).old"
    }
    Copy-Item -Force $file.FullName $to
}

# スタートメニューに「AsIs」を置く(前の名前「asis」のものは消す)
$menu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\AsIs.lnk'
Remove-Item -Force $menu -ErrorAction SilentlyContinue
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

Write-Host "AsIs を入れました: $dir"
Write-Host 'スタートメニューの「AsIs」から開けます。'
if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    Write-Host ''
    Write-Host 'GitHub の公式コマンド gh が見つかりません。AsIs は gh のログインを借りて読みます。'
    Write-Host '  winget install GitHub.cli   のあと   gh auth login'
}
