#!/usr/bin/env bash
# Trusted main only, after all package checks and the existing CI/Nix workflows.
set -euo pipefail
assets=$(realpath "${1:?}")
commit=${2:?}
: "${GH_REPO:?}"
[[ "$commit" =~ ^[0-9a-f]{40}$ ]]
(cd "$assets" && sha256sum --check SHA256SUMS)
python3 - "$assets/source.json" "$commit" <<'PY'
import json, sys
assert json.load(open(sys.argv[1]))['commit'] == sys.argv[2]
PY
files=(biri-debian13-amd64.deb biri-fedora44-x86_64.rpm biri-arch-x86_64.pkg.tar.zst
       biri-tumbleweed-x86_64.rpm biri-void-glibc-x86_64.tar.gz
       biri-debian-source.tar.gz biri-source.tar.gz source.json SHA256SUMS)
for file in "${files[@]}"; do test -s "$assets/$file"; done
test -s "$assets/release-notes.md"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if gh api "repos/$GH_REPO/releases/tags/development" > "$work/release.json" 2> "$work/error"; then
    python3 - "$work/release.json" <<'PY'
import json, sys
release = json.load(open(sys.argv[1]))
assert release['prerelease'] and not release.get('immutable', False), 'Expected a mutable development prerelease'
PY
    relationship=$(gh api "repos/$GH_REPO/compare/development...$commit" --jq .status)
    case "$relationship" in
        ahead|identical) ;;
        behind) echo 'Keeping the newer published development build.'; exit 0 ;;
        *) echo 'Development history diverged; refusing to overwrite it.' >&2; exit 1 ;;
    esac
    # Upload checksums last so a partially updated download set is detectable.
    for file in "${files[@]}"; do gh release upload development "$assets/$file" --clobber; done
    gh api --method PATCH "repos/$GH_REPO/git/refs/tags/development" -f sha="$commit" -F force=true > /dev/null
    gh release edit development --title 'Latest development build' --prerelease --latest=false \
        --notes-file "$assets/release-notes.md" --draft=false
elif grep -Fq '(HTTP 404)' "$work/error"; then
    paths=()
    for file in "${files[@]}"; do paths+=("$assets/$file"); done
    gh release create development "${paths[@]}" --target "$commit" \
        --title 'Latest development build' --prerelease --latest=false --notes-file "$assets/release-notes.md"
    gh api --method PATCH "repos/$GH_REPO/git/refs/tags/development" -f sha="$commit" -F force=true > /dev/null
else
    cat "$work/error" >&2
    exit 1
fi
