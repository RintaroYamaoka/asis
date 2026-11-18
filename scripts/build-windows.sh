#!/bin/bash
# Windows に渡す 1 つの zip を作る(Linux・WSL の上で)。出来るもの: dist/asis-windows.zip
# 要るもの: rustup target add x86_64-pc-windows-gnu / mingw-w64(x86_64-w64-mingw32-gcc と windres)
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target x86_64-pc-windows-gnu -p asis -p asis-gui
out=dist/asis-windows
rm -rf "$out" dist/asis-windows.zip
mkdir -p "$out"
cp target/x86_64-pc-windows-gnu/release/asis-gui.exe target/x86_64-pc-windows-gnu/release/asis.exe "$out/"
cp windows/install.ps1 windows/uninstall.ps1 "$out/"
python3 -c "import shutil; shutil.make_archive('dist/asis-windows', 'zip', 'dist', 'asis-windows')"
ls -la dist/asis-windows.zip
