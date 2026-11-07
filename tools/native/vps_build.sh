#!/bin/bash
# Builds the Native client for Windows on a Linux box instead of GitHub Actions: the newest upstream
# Nightly's source, patched exactly as .github/workflows/native-build.yml does, cross-compiled for
# x86_64-pc-windows-gnu with mingw-w64.
#
# Usage: vps_build.sh [branch]        (default: the branch this script is on, else native/skins-capes)
# Needs: rustup (+ target x86_64-pc-windows-gnu), gcc-mingw-w64-x86-64 (posix threads), binutils-mingw-w64-x86-64,
#        nasm, cmake, python3 with pillow, git, curl.
# Result: $WORK/dist/{horizon-client-windows-x64.exe, SHA256SUMS, compat.json, tag}, ready for
#        tools/native/vps_release.py. Nothing is published here.
set -euo pipefail
. "$HOME/.cargo/env"

BRANCH="${1:-native/skins-capes}"
WORK="${NATIVE_BUILD_DIR:-$HOME/native-rust}"
UPSTREAM="horizon-rust/horizon"
TARGET="x86_64-pc-windows-gnu"
JOBS="${CARGO_BUILD_JOBS:-3}"
mkdir -p "$WORK"
cd "$WORK"

# The build may be killed first if memory runs out, never the other services on the machine.
echo 1000 > /proc/self/oom_score_adj 2> /dev/null || true

# This repository (the patches), from GitHub.
if [ ! -d native/.git ]; then
  git clone -q "https://github.com/atlas-thedev/horizon.git" native
fi
git -C native fetch -q origin "$BRANCH"
git -C native checkout -q -B "$BRANCH" "origin/$BRANCH"
echo "patches: $(git -C native log -1 --format='%h %s')"

# The newest upstream Nightly, the same way the workflow names it.
read -r DATE SHORT NAME < <(curl -fsS "https://api.github.com/repos/$UPSTREAM/releases/tags/nightly" | python3 -c '
import json, re, sys
name = json.load(sys.stdin)["name"]
m = re.match(r"^Nightly (\d{4}-\d{2}-\d{2}) · ([0-9a-f]+)$", name)
if not m:
    raise SystemExit("Unexpected Nightly title: %r" % name)
print(m.group(1), m.group(2), name.replace(" ", "_"))
')
NAME="${NAME//_/ }"
TAG="native-$DATE-$SHORT"
echo "nightly: $NAME -> $TAG"

# Its source (kept between builds, so only what changed is compiled again).
if [ ! -d src/.git ]; then
  git clone -q --filter=blob:none "https://github.com/$UPSTREAM.git" src
fi
# --force: upstream moves its nightly tags (nightly, nightly-<date>) to each new build
git -C src fetch -q --tags --force
git -C src checkout -q -f "nightly-source/$DATE-$SHORT" 2> /dev/null || git -C src checkout -q -f "$SHORT"
git -C src clean -qfd crates packaging tools

# The patches, in the workflow's order.
python3 native/tools/native/rebrand.py src
base64 -d native/packaging/native/icon64.png.b64 > src/crates/horizon-theme/icons/horizon-64.png
python3 native/tools/native/boot_screen.py src native
python3 native/tools/native/native_api.py src native
python3 native/tools/native/native_skins.py src native
python3 native/tools/native/native_marks.py src native
python3 native/tools/native/native_cosmetics.py src native
python3 native/tools/native/native_ui.py src native
python3 native/tools/native/native_tweaks.py src native
python3 native/tools/native/native_client.py src native
python3 native/tools/native/make_vanilla_icon.py src/packaging/icons/horizon.ico src/crates/horizon-theme/icons/horizon-64.png

cd src
export RUSTFLAGS="-C target-feature=+crt-static"
export HORIZON_VERSION="nightly-$DATE"
export HORIZON_NIGHTLY="$DATE $SHORT"
nice -n 15 cargo build --release -j "$JOBS" --target "$TARGET" \
  -p horizon-client -p horizon-client-lib --bin horizon-client

EXE="target/$TARGET/release/horizon-client.exe"
python3 tools/check_imports.py "$EXE"

cd "$WORK"
rm -rf dist
mkdir dist
cp "src/$EXE" dist/horizon-client-windows-x64.exe
(cd dist && sha256sum horizon-client-windows-x64.exe > SHA256SUMS)
printf '%s\n' "{\"upstream_release\":\"$NAME\",\"upstream_commit\":\"$SHORT\"}" > dist/compat.json
echo "$TAG" > dist/tag
echo "built $TAG: $(du -h dist/horizon-client-windows-x64.exe | cut -f1)"
