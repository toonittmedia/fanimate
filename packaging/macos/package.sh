#!/usr/bin/env bash
# Build Fanimate.app, its disk image and its update archive.
#
#   packaging/macos/package.sh <version> <release-folder> <out-folder> <arch>
#
# <release-folder> holds fanimate, fanimate-cli, rhubarb/ and the
# licence files. Makes:
#   Fanimate-<version>-macos-<arch>.dmg           to install by dragging
#   Fanimate-<version>-macos-<arch>.app.tar.gz    what the updater installs
# The app is not signed or notarised yet (that needs an Apple Developer
# account): the first start needs right-click > Open.
set -euo pipefail

version="$1"
src="$(cd "$2" && pwd)"
out="$(mkdir -p "$3" && cd "$3" && pwd)"
arch="$4"
root="$(cd "$(dirname "$0")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

app="$work/Fanimate.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# the programs beside the app's executable, where Fanimate looks for them
cp -r "$src"/. "$app/Contents/MacOS/"
sed "s/@VERSION@/$version/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$root/brand/app-icon/fanimate.icns" "$app/Contents/Resources/fanimate.icns"
cp "$root/brand/file-icon/fnm-file.icns" "$app/Contents/Resources/fnm-file.icns"

tar -czf "$out/Fanimate-$version-macos-$arch.app.tar.gz" -C "$work" Fanimate.app

stage="$work/dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "Fanimate $version" -srcfolder "$stage" -ov -format UDZO \
  "$out/Fanimate-$version-macos-$arch.dmg"

ls -l "$out"
