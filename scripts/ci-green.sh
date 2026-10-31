#!/usr/bin/env bash
# Ждёт, пока Clippy Lint и Frontend Check коммита <sha> на main закончатся, и
# требует, чтобы оба были зелёными. Нужны GH_TOKEN (actions: read) и
# GITHUB_REPOSITORY. Проверки на push идут только по вершине пуша: тег ставится
# на последний запушенный в main коммит.
set -euo pipefail

sha="${1:?usage: ci-green.sh <sha>}"
deadline=$((SECONDS + 110 * 60))
appear_by=$((SECONDS + 15 * 60))

for workflow in lint-clippy.yml frontend-check.yml; do
  while :; do
    run=$(gh api "repos/${GITHUB_REPOSITORY}/actions/workflows/$workflow/runs?head_sha=$sha&event=push&per_page=20" \
      --jq '.workflow_runs | sort_by(.created_at) | last | if . == null then "" else "\(.status) \(.conclusion) \(.html_url)" end')
    if [ -z "$run" ]; then
      if [ "$SECONDS" -ge "$appear_by" ]; then
        echo "::error::$workflow never ran for $sha — tag the last commit pushed to main: push checks run only for the tip of a push"
        exit 1
      fi
    else
      read -r status conclusion url <<< "$run"
      if [ "$status" = "completed" ]; then
        if [ "$conclusion" = "success" ]; then
          echo "$workflow: success ($url)"
          break
        fi
        echo "::error::$workflow ended with '$conclusion' for $sha: $url"
        exit 1
      fi
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "::error::$workflow did not finish in time for $sha"
      exit 1
    fi
    sleep 30
  done
done
