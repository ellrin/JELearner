#!/bin/sh
# Publish the iPad app (ipad/) to GitHub Pages: the gh-pages branch, folder app/.
# The page holds code only; each iPad keeps its own repository, token and key.
#
#   scripts/publish-ipad.sh            # → https://<owner>.github.io/<repo>/app/
set -e
cd "$(git rev-parse --show-toplevel)"
SRC=$(pwd)/ipad
WT=$(mktemp -d)/pages
git fetch -q origin gh-pages
git worktree add -q "$WT" origin/gh-pages
trap 'git worktree remove --force "$WT"' EXIT
rm -rf "$WT/app"
mkdir -p "$WT/app"
cp "$SRC"/index.html "$SRC"/app.js "$SRC"/style.css "$SRC"/sw.js "$SRC"/manifest.webmanifest "$SRC"/icon-256.png "$SRC"/icon-512.png "$WT/app/"
mkdir -p "$WT/app/vendor"
cp "$SRC"/vendor/*.js "$WT/app/vendor/"
# A new cache name on every publish, so iPads pick up the new version.
sed -i.bak "s/const CACHE = \"jl-app-v[^\"]*\"/const CACHE = \"jl-app-$(git rev-parse --short HEAD)\"/" "$WT/app/sw.js" && rm "$WT/app/sw.js.bak"
cd "$WT"
git add -A app
if git diff --cached --quiet; then echo "nothing new"; exit 0; fi
git commit -q -m "iPad app $(git -C "$SRC" rev-parse --short HEAD)"
git push -q origin HEAD:gh-pages
echo "published: app/"
