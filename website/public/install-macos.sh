#!/bin/bash
set -euo pipefail

app_name="Clod Clash.app"
dest="/Applications/$app_name"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Этот скрипт только для macOS." >&2
  exit 1
fi

if [ "$(sysctl -in hw.optional.arm64)" = "1" ]; then
  arch="aarch64"
else
  arch="x64"
fi

if pgrep -xq clod-clash; then
  echo "Clod Clash запущен. Закройте приложение и повторите установку." >&2
  exit 1
fi

tmp="$(mktemp -d)"
mnt="$tmp/mnt"
cleanup() {
  hdiutil detach -quiet "$mnt" 2>/dev/null || true
  rm -rf "$tmp"
}
trap cleanup EXIT

url="https://github.com/Mrvibecodic/clod-clash/releases/latest/download/Clod.Clash_${arch}.dmg"
echo "Скачиваю $url"
curl -fL --retry 3 --progress-bar -o "$tmp/app.dmg" "$url"

mkdir "$mnt"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$tmp/app.dmg"

sudo=""
if [ ! -w /Applications ]; then
  sudo="sudo"
fi

echo "Ставлю в $dest"
ditto "$mnt/$app_name" "$tmp/$app_name"
$sudo rm -rf "$dest"
$sudo mv "$tmp/$app_name" "$dest"

echo "Готово."
open "$dest"
