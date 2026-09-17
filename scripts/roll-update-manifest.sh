#!/usr/bin/env bash
# Раскладывает latest.json опубликованного релиза по каналам обновления:
# в сам релиз, в `updater-prerelease` и, для версии без дефиса, в `updater`.
# Манифест канала, где уже лежит версия новее, не трогается.
#
# Вызов: roll-update-manifest.sh <id релиза> <версия>
# Окружение: GH_TOKEN, GITHUB_REPOSITORY. Запускать из корня репозитория.
set -euo pipefail

RELEASE_ID="$1"
VERSION="${2#v}"
REPO="$GITHUB_REPOSITORY"

# Истина, когда версия $1 старше версии $2.
older_than() {
  local a="$1" b="$2" a_core="${1%%-*}" b_core="${2%%-*}" first
  if [ "$a" = "$b" ]; then
    return 1
  fi
  if [ "$a_core" != "$b_core" ]; then
    first=$(printf '%s\n%s\n' "$a_core" "$b_core" | sort -V | head -1)
    [ "$first" = "$a_core" ]
    return
  fi
  case "$a" in *-*) ;; *) return 1 ;; esac
  case "$b" in *-*) ;; *) return 0 ;; esac
  first=$(printf '%s\n%s\n' "${a#*-}" "${b#*-}" | sort -V | head -1)
  [ "$first" = "${a#*-}" ]
}

ensure_release() {
  local tag="$1" name="$2" body="$3" id
  id=$(gh api "repos/$REPO/releases/tags/$tag" --jq .id 2>/dev/null || true)
  case "$id" in
    '' | *[!0-9]*) id="" ;;
  esac
  if [ -z "$id" ]; then
    id=$(gh api -X POST "repos/$REPO/releases" \
      -f tag_name="$tag" -f name="$name" -f body="$body" \
      -F draft=false -F prerelease=true --jq .id)
  fi
  echo "$id"
}

manifest_asset() {
  gh api "repos/$REPO/releases/$1/assets?per_page=100" \
    --jq '.[] | select(.name=="latest.json") | .id' | head -1
}

upload_manifest() {
  gh api -X POST \
    -H "Content-Type: application/json" \
    --input latest.json \
    "https://uploads.github.com/repos/$REPO/releases/$1/assets?name=latest.json" \
    --jq .state
}

put_manifest() {
  local id="$1" channel="$2" old current
  old=$(manifest_asset "$id")
  if [ -n "$old" ]; then
    current=$(gh api -H "Accept: application/octet-stream" "repos/$REPO/releases/assets/$old" \
      | jq -r '.version // empty' 2>/dev/null || true)
    current="${current#v}"
    if [ -n "$current" ] && older_than "$VERSION" "$current"; then
      echo "$channel already serves $current, newer than $VERSION: left as is"
      return 0
    fi
    gh api -X DELETE "repos/$REPO/releases/assets/$old"
  fi
  upload_manifest "$id"
  echo "latest.json rolled into $channel"
}

gh api "repos/$REPO/releases/$RELEASE_ID/assets?per_page=100" > assets.json
asset_id=$(jq -r '.[] | select(.name=="latest.json") | .id' assets.json | head -1)
if [ -z "$asset_id" ]; then
  echo "::error::release $RELEASE_ID has no latest.json"
  exit 1
fi
gh api -H "Accept: application/octet-stream" "repos/$REPO/releases/assets/$asset_id" > latest.json
jq -e .version latest.json

# Пустые notes отключают кнопку Update у старых клиентов, запасной текст
# обязателен.
notes=$(awk -v ver="## v$VERSION" '
  $0 == ver { grab = 1; next }
  /^## v/ || /^---/ { if (grab) exit }
  grab { print }
' UPDATELOG.md)
if [ -z "$notes" ]; then
  echo "::warning::UPDATELOG.md has no section for v$VERSION"
  notes="New version $VERSION is available."
fi

# tauri-action пишет манифест, пока релиз ещё черновик, со ссылками
# api.github.com. Апдейтер качает без заголовков API, ему нужны публичные.
jq --arg notes "$notes" --slurpfile assets assets.json '
  .notes = $notes
  | .platforms |= with_entries(
      .value.url = (
        if (.value.url | test("api\\.github\\.com/.*/assets/[0-9]+$")) then
          ((.value.url | capture("assets/(?<id>[0-9]+)$").id | tonumber) as $id
           | ($assets[0][] | select(.id == $id) | .browser_download_url) // .value.url)
        else
          .value.url
        end
      )
    )' latest.json > latest.patched.json
jq -e .version latest.patched.json
mv latest.patched.json latest.json
echo "patched latest.json:"
jq '{version, notes, url_sample: (.platforms | to_entries | .[0].value.url)}' latest.json

gh api -X DELETE "repos/$REPO/releases/assets/$asset_id"
upload_manifest "$RELEASE_ID"

pre=$(ensure_release updater-prerelease "Pre-release update manifest" \
  "Service release: latest.json of the newest build, pre-releases included. Do not download installers from here.")
put_manifest "$pre" updater-prerelease

case "$VERSION" in
  *-*)
    echo "$VERSION is a pre-release, the stable updater manifest stays untouched"
    exit 0
    ;;
esac

stable=$(ensure_release updater "Update manifest" \
  "Service release: latest.json of the newest stable build. Do not download installers from here.")
put_manifest "$stable" updater
