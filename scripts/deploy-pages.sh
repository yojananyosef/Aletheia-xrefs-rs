#!/usr/bin/env bash
# Build the release bundle and publish it to the gh-pages branch.
# Usage: scripts/deploy-pages.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
remote="$(git -C "$root" remote get-url origin)"
pages="$(mktemp -d)"
trap 'rm -rf "$pages"' EXIT

cd "$root/crates/app"
trunk build --release

git clone --quiet --no-checkout "$remote" "$pages"
cd "$pages"
git checkout --orphan gh-pages >/dev/null 2>&1
git rm -rf . >/dev/null 2>&1 || true
cp -a "$root/crates/app/dist/." .
touch .nojekyll
git add -A
git -c user.name="${GIT_AUTHOR_NAME:-Johan Gutierrez}" \
    -c user.email="${GIT_AUTHOR_EMAIL:-johan.gutierrez@alu.unach.cl}" \
    commit -m "Deploy $(date -u +%Y-%m-%dT%H:%M:%SZ)"
git push --force origin gh-pages

echo "Published $(du -sh "$root/crates/app/dist" | cut -f1) to gh-pages."
