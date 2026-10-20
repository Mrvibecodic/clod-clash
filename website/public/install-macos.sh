#!/bin/bash
set -euo pipefail

app_name="Clod Clash.app"
dest="/Applications/$app_name"

lang=""
if [ "$(uname -s)" = "Darwin" ]; then
  lang="$(defaults read -g AppleLanguages 2>/dev/null | awk 'NR == 2 { gsub(/[ ",]/, ""); print; exit }' || true)"
fi
lang="${lang:-${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}}"
case "${lang%%[-_.]*}" in
  ru | uk | be | kk | uz | ky | tg) ru=1 ;;
  *) ru=0 ;;
esac
say() {
  if [ "$ru" = 1 ]; then echo "$1"; else echo "$2"; fi
}

if [ "$(uname -s)" != "Darwin" ]; then
  say "Этот скрипт только для macOS." "This script is for macOS only." >&2
  exit 1
fi

if [ "$(sysctl -in hw.optional.arm64)" = "1" ]; then
  arch="aarch64"
else
  arch="x64"
fi

if pgrep -xq clod-clash; then
  say "Clod Clash запущен. Закройте приложение и повторите установку." \
    "Clod Clash is running. Quit the app and run the installer again." >&2
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
say "Скачиваю $url" "Downloading $url"
curl -fL --retry 3 --progress-bar -o "$tmp/app.dmg" "$url"

mkdir "$mnt"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$tmp/app.dmg"

sudo=""
if [ ! -w /Applications ]; then
  sudo="sudo"
fi

say "Ставлю в $dest" "Installing to $dest"
ditto "$mnt/$app_name" "$tmp/$app_name"
$sudo rm -rf "$dest"
$sudo mv "$tmp/$app_name" "$dest"

say "Готово." "Done."
open "$dest"
