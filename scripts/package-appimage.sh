#!/usr/bin/env bash
# Build a ReelMux AppImage with GTK4 bundled through linuxdeploy.

set -euo pipefail

binary="${1:?usage: package-appimage.sh <binary> <version> <arch>}"
version="${2:?usage: package-appimage.sh <binary> <version> <arch>}"
arch="${3:?usage: package-appimage.sh <binary> <version> <arch>}"

if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "error: version must use MAJOR.MINOR.PATCH (got '$version')" >&2
  exit 1
fi

case "$arch" in
  amd64)
    linuxdeploy_arch="x86_64"
    linuxdeploy_sha256="c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d"
    runtime_sha256="1cc49bcf1e2ccd593c379adb17c9f85a36d619088296504de95b1d06215aebbf"
    ;;
  arm64)
    linuxdeploy_arch="aarch64"
    linuxdeploy_sha256="620095110d693282b8ebeb244a95b5e911cf8f65f76c88b4b47d16ae6346fcff"
    runtime_sha256="7d5d772b7c32f0c84caf0a452a3072a5709027d7eac5856feb89a7a7a8881372"
    ;;
  *)
    echo "error: arch must be amd64 or arm64 (got '$arch')" >&2
    exit 1
    ;;
esac

cd "$(git rev-parse --show-toplevel)"

if [[ ! -x "$binary" ]]; then
  echo "error: release binary is missing or not executable: $binary" >&2
  exit 1
fi

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

require_command curl
require_command patch
require_command sha256sum

linuxdeploy_version="1-alpha-20251107-1"
gtk_plugin_commit="7a3fbc31a9e5075073ff8790f26effbac5f84453"
gtk_plugin_sha256="b0f4cbc684a0103a9651f0955b635eaea0096b3a66c0f5a2c2aa337960375171"
mkdir -p target
stage="$(mktemp -d target/appimage-stage.XXXXXX)"
trap 'rm -rf "$stage"' EXIT

linuxdeploy="$stage/linuxdeploy-${linuxdeploy_arch}.AppImage"
gtk_plugin="$stage/linuxdeploy-plugin-gtk.sh"
runtime="$stage/runtime-${linuxdeploy_arch}"
curl -fsSL \
  "https://github.com/linuxdeploy/linuxdeploy/releases/download/${linuxdeploy_version}/linuxdeploy-${linuxdeploy_arch}.AppImage" \
  -o "$linuxdeploy"
curl -fsSL \
  "https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/${gtk_plugin_commit}/linuxdeploy-plugin-gtk.sh" \
  -o "$gtk_plugin"
curl -fsSL \
  "https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-${linuxdeploy_arch}" \
  -o "$runtime"

printf '%s  %s\n' "$linuxdeploy_sha256" "$linuxdeploy" | sha256sum --check --status
printf '%s  %s\n' "$gtk_plugin_sha256" "$gtk_plugin" | sha256sum --check --status
printf '%s  %s\n' "$runtime_sha256" "$runtime" | sha256sum --check --status
chmod +x "$linuxdeploy" "$gtk_plugin"
patch --silent "$gtk_plugin" packaging/linuxdeploy-plugin-gtk-moduleless-pixbuf.patch

mkdir -p dist
output="$(pwd)/dist/reelmux_${version}_linux_${arch}.AppImage"
appdir="$stage/ReelMux.AppDir"
install -Dm644 data/io.github.nahime0.ReelMux.metainfo.xml \
  "$appdir/usr/share/metainfo/io.github.nahime0.ReelMux.appdata.xml"

APPIMAGE_EXTRACT_AND_RUN=1 \
DEPLOY_GTK_VERSION=4 \
LDAI_OUTPUT="$output" \
LDAI_RUNTIME_FILE="$runtime" \
LINUXDEPLOY_OUTPUT_VERSION="$version" \
NO_STRIP=1 \
"$linuxdeploy" \
  --appdir "$appdir" \
  --executable "$binary" \
  --desktop-file data/io.github.nahime0.ReelMux.desktop \
  --icon-file data/io.github.nahime0.ReelMux.svg \
  --plugin gtk \
  --output appimage

chmod +x "$output"
echo "$output"
