#!/usr/bin/env bash
# Build Fanimate's Linux packages from a release folder.
#
#   packaging/linux/package.sh <version> <release-folder> <out-folder>
#
# <release-folder> holds fanimate, fanimate-cli, rhubarb/ and the
# licence files, as the release workflow assembles them. Makes:
#   Fanimate-<version>-linux-x86_64.AppImage   updates itself
#   fanimate_<version>_amd64.deb               installs to /opt/fanimate
# and prepares flatpak-src/ for the Flatpak bundle (see the manifest).
set -euo pipefail

version="$1"
src="$(cd "$2" && pwd)"
out="$(mkdir -p "$3" && cd "$3" && pwd)"
root="$(cd "$(dirname "$0")/../.." && pwd)"
here="$root/packaging/linux"
id=io.github.toonittmedia.Fanimate
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# --- AppImage -----------------------------------------------------------
app="$work/Fanimate.AppDir"
mkdir -p "$app/usr/lib/fanimate" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
cp -r "$src"/. "$app/usr/lib/fanimate/"
cp "$here/$id.desktop" "$app/$id.desktop"
cp "$here/$id.desktop" "$app/usr/share/applications/"
cp "$root/brand/app-icon/fanimate-256.png" "$app/$id.png"
cp "$root/brand/app-icon/fanimate-256.png" "$app/usr/share/icons/hicolor/256x256/apps/$id.png"
mkdir -p "$app/usr/share/mime/packages" "$app/usr/share/icons/hicolor/512x512/mimetypes"
cp "$here/$id.xml" "$app/usr/share/mime/packages/"
cp "$root/brand/file-icon/fnm-file-512.png" "$app/usr/share/icons/hicolor/512x512/mimetypes/application-x-fanimate-project.png"
cat > "$app/AppRun" <<'EOF'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/lib/fanimate/fanimate" "$@"
EOF
chmod +x "$app/AppRun"
tool="$work/appimagetool"
# pinned release of the AppImage project's own tool (MIT)
curl -fsSL -o "$tool" \
  https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
chmod +x "$tool"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$out/Fanimate-$version-linux-x86_64.AppImage"

# --- .deb ---------------------------------------------------------------
deb="$work/deb"
mkdir -p "$deb/DEBIAN" "$deb/opt/fanimate" "$deb/usr/bin" "$deb/usr/share/applications" \
  "$deb/usr/share/icons/hicolor/256x256/apps" "$deb/usr/share/metainfo" \
  "$deb/usr/share/mime/packages" "$deb/usr/share/icons/hicolor/512x512/mimetypes"
cp -r "$src"/. "$deb/opt/fanimate/"
ln -s /opt/fanimate/fanimate "$deb/usr/bin/fanimate"
ln -s /opt/fanimate/fanimate-cli "$deb/usr/bin/fanimate-cli"
cp "$here/$id.desktop" "$deb/usr/share/applications/"
cp "$here/$id.metainfo.xml" "$deb/usr/share/metainfo/"
# dpkg's shared-mime-info trigger rebuilds the MIME database for this file
cp "$here/$id.xml" "$deb/usr/share/mime/packages/"
cp "$root/brand/file-icon/fnm-file-512.png" "$deb/usr/share/icons/hicolor/512x512/mimetypes/application-x-fanimate-project.png"
cp "$root/brand/app-icon/fanimate-256.png" "$deb/usr/share/icons/hicolor/256x256/apps/$id.png"
size=$(du -sk "$deb" | cut -f1)
cat > "$deb/DEBIAN/control" <<EOF
Package: fanimate
Version: $version
Section: graphics
Priority: optional
Architecture: amd64
Installed-Size: $size
Depends: libc6 (>= 2.35), libgtk-3-0 | libgtk-3-0t64, libasound2 | libasound2t64, libxkbcommon0, libwayland-client0, libgl1
Recommends: mesa-vulkan-drivers
Suggests: ffmpeg
Maintainer: The Fanimate Contributors <noreply@github.com>
Homepage: https://github.com/toonittmedia/fanimate
Description: Free 2D character animation
 Bone rigging with inverse kinematics and Smart Bones, vector drawing,
 frame-by-frame animation, lip sync and video export. Opens Moho and
 Adobe Animate projects.
EOF
dpkg-deb --root-owner-group --build "$deb" "$out/fanimate_${version}_amd64.deb"

# --- Flatpak source -----------------------------------------------------
flat="$root/flatpak-src"
rm -rf "$flat"
mkdir -p "$flat"
cp -r "$src"/. "$flat/"
cp "$here/$id.desktop" "$here/$id.metainfo.xml" "$here/$id.xml" "$flat/"
cp "$root/brand/app-icon/fanimate-256.png" "$flat/icon-256.png"
cp "$root/brand/file-icon/fnm-file-512.png" "$flat/fnm-file-512.png"

ls -l "$out"
