#!/usr/bin/env bash
#
# release.sh — bump the version, commit it, and push a version tag to GitHub.
#
# Pushing the tag triggers .github/workflows/release.yml, which builds the
# Tauri app for Windows + Linux and drafts a GitHub release.
#
# Usage:
#   ./release.sh              # bump patch  (0.1.0 -> 0.1.1)
#   ./release.sh minor        # bump minor  (0.1.0 -> 0.2.0)
#   ./release.sh major        # bump major  (0.1.0 -> 1.0.0)
#   ./release.sh 1.2.3        # set an exact version
#
# Environment:
#   REMOTE   git remote to push to (default: origin)

set -euo pipefail
cd "$(dirname "$0")"

REMOTE="${REMOTE:-origin}"
CARGO_TOML="Cargo.toml"
TAURI_CONF="tauri.conf.json"
CARGO_LOCK="Cargo.lock"

info() { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# --- version helpers ---------------------------------------------------------

# Package version = first `version = "..."` line in Cargo.toml.
current_version() {
  sed -n 's/^version = "\([^"]*\)".*/\1/p' "$CARGO_TOML" | head -n1
}

is_semver() { [[ "$1" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; }

bump() {
  local IFS=.
  local -a v=($1)
  case "$2" in
    major) v=( $((v[0]+1)) 0 0 ) ;;
    minor) v=( "${v[0]}" $((v[1]+1)) 0 ) ;;
    patch) v=( "${v[0]}" "${v[1]}" $((v[2]+1)) ) ;;
  esac
  echo "${v[0]}.${v[1]}.${v[2]}"
}

# --- resolve target version --------------------------------------------------

CURRENT="$(current_version)"
[[ -n "$CURRENT" ]] || die "could not read version from $CARGO_TOML"
is_semver "$CURRENT" || die "unexpected current version: '$CURRENT'"

case "${1:-patch}" in
  patch|minor|major) NEW="$(bump "$CURRENT" "$1")" ;;
  *)
    NEW="$1"
    is_semver "$NEW" || die "usage: $0 [patch|minor|major|X.Y.Z]  (got '$1')"
    ;;
esac

[[ "$NEW" != "$CURRENT" ]] || die "new version equals current version ($CURRENT)"
TAG="v$NEW"

info "bumping version: $CURRENT -> $NEW  (tag $TAG)"

# --- update version in the three files ---------------------------------------

# Cargo.toml — replace only the first `version = "..."` line (the package version).
awk -v new="$NEW" '
  !done && /^version = "/ { print "version = \"" new "\""; done=1; next }
  { print }
' "$CARGO_TOML" > "$CARGO_TOML.tmp" && mv "$CARGO_TOML.tmp" "$CARGO_TOML"

# tauri.conf.json
sed -i "s/\"version\": \"$CURRENT\"/\"version\": \"$NEW\"/" "$TAURI_CONF"

# Cargo.lock — update the version right after this package's name line.
if [[ -f "$CARGO_LOCK" ]]; then
  PKG_NAME="$(sed -n 's/^name = "\([^"]*\)".*/\1/p' "$CARGO_TOML" | head -n1)"
  awk -v pkg="$PKG_NAME" -v new="$NEW" '
    /^name = / { in_target = ($0 == "name = \"" pkg "\"") }
    in_target && /^version = "/ { print "version = \"" new "\""; in_target = 0; next }
    { print }
  ' "$CARGO_LOCK" > "$CARGO_LOCK.tmp" && mv "$CARGO_LOCK.tmp" "$CARGO_LOCK"
fi

# --- commit + tag + push -----------------------------------------------------

if ! git remote get-url "$REMOTE" >/dev/null 2>&1; then
  die "no git remote '$REMOTE'. Add it first, e.g.:\n     git remote add $REMOTE git@github.com:YOU/REPO.git"
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
if [[ -z "$BRANCH" || "$BRANCH" == "HEAD" ]]; then
  die "not on a branch (detached HEAD). Check out a branch first."
fi

if ! git rev-parse --verify HEAD >/dev/null 2>&1; then
  info "note: repository has no commits yet — only the version files will be committed."
fi

git add "$CARGO_TOML" "$TAURI_CONF"
[[ -f "$CARGO_LOCK" ]] && git add "$CARGO_LOCK"

git commit -m "release: $TAG"
git tag -a "$TAG" -m "release $TAG"

info "pushing $BRANCH and tag $TAG to $REMOTE ..."
git push "$REMOTE" "$BRANCH"
git push "$REMOTE" "$TAG"

info "done. GitHub Actions will now build and draft the release for $TAG."
