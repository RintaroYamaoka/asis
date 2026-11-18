#!/bin/bash
# WSL の上で打つ。Windows 版を作って、この PC の Windows に入れる。
# 入れたあとは、WSL で `asis-gui` と打っても、スタートメニューの「asis」からでも、Windows 版の窓が開く
# (gh のログイン・設定・手元の作業は、WSL の中のものをそのまま使う)。
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/build-windows.sh
cd /mnt/c
powershell.exe -NoProfile -ExecutionPolicy Bypass \
  -File "$(wslpath -w "$OLDPWD/dist/asis-windows/install.ps1")" -Wsl "$WSL_DISTRO_NAME"
