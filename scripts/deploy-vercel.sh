#!/usr/bin/env bash
# Build the release bundle and publish it to the Vercel project `aletheia-arcs`.
# Usage: scripts/deploy-vercel.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

cd "$root/crates/app"
trunk build --release

cd dist
vercel link --yes --project aletheia-arcs >/dev/null
vercel deploy --prod --yes --archive=tgz

echo "Published $(du -sh . | cut -f1) to https://arcs.johan.cl/"
