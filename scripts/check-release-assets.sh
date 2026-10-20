#!/usr/bin/env bash
# Проверяет, что релиз собран целиком: файлы всех целей под точными именами
# (с версией в имени или уже без неё) и latest.json со всеми платформами.
#
# Вызов: check-release-assets.sh <id релиза> <версия>
# Окружение: GH_TOKEN, GITHUB_REPOSITORY. Запускать из корня репозитория.
set -euo pipefail

RELEASE_ID="$1"
VERSION="${2#v}"
REPO="$GITHUB_REPOSITORY"

gh api "repos/$REPO/releases/$RELEASE_ID/assets?per_page=100" > release-assets.json
echo "assets:"
jq -r '.[].name' release-assets.json

names=$(jq -r '.[].name' release-assets.json | while read -r name; do
  name="${name/_${VERSION}_/_}"
  echo "${name/-${VERSION}-1./-}"
done)

missing=0
for file in latest.json \
  Clod.Clash_x64-setup.exe Clod.Clash_x64_portable.zip \
  Clod.Clash_aarch64.dmg Clod.Clash_x64.dmg \
  Clod.Clash_amd64.deb Clod.Clash-x86_64.rpm; do
  if ! grep -qxF -- "$file" <<< "$names"; then
    echo "::error::Release $RELEASE_ID has no $file — сборка не закончилась?"
    missing=1
  fi
done
if [ "$missing" != 0 ]; then
  exit 1
fi

manifest=$(jq -r '.[] | select(.name=="latest.json") | .id' release-assets.json | head -1)
gh api -H "Accept: application/octet-stream" "repos/$REPO/releases/assets/$manifest" > release-latest.json
for platform in windows-x86_64 darwin-aarch64 darwin-x86_64 linux-x86_64; do
  if ! jq -e --arg p "$platform" \
    '.platforms | keys | any(. == $p or startswith($p + "-"))' release-latest.json > /dev/null; then
    echo "::error::latest.json of release $RELEASE_ID has no platform $platform"
    missing=1
  fi
done
exit "$missing"
