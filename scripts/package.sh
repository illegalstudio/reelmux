#!/usr/bin/env bash
# Package one ReelMux Linux binary as a tarball and, when available, a deb.

set -euo pipefail

binary="${1:?usage: package.sh <binary> <version> <arch>}"
version="${2:?usage: package.sh <binary> <version> <arch>}"
arch="${3:?usage: package.sh <binary> <version> <arch>}"

if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "error: version must use MAJOR.MINOR.PATCH (got '$version')" >&2
  exit 1
fi

case "$arch" in
  amd64|arm64) ;;
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

mkdir -p dist
base="reelmux_${version}_linux_${arch}"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

archive_root="$stage/$base"
install -Dm755 "$binary" "$archive_root/bin/reelmux"
install -Dm644 data/io.github.nahime0.ReelMux.desktop \
  "$archive_root/share/applications/io.github.nahime0.ReelMux.desktop"
install -Dm644 data/io.github.nahime0.ReelMux.svg \
  "$archive_root/share/icons/hicolor/scalable/apps/io.github.nahime0.ReelMux.svg"
install -Dm644 LICENSE "$archive_root/LICENSE"
install -Dm644 README.md "$archive_root/README.md"
install -Dm644 THIRD_PARTY.md "$archive_root/THIRD_PARTY.md"

tar -C "$stage" -czf "dist/${base}.tar.gz" "$base"
echo "dist/${base}.tar.gz"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb not found; skipping the deb package" >&2
  exit 0
fi

deb_root="$stage/deb"
install -Dm755 "$binary" "$deb_root/usr/bin/reelmux"
install -Dm644 data/io.github.nahime0.ReelMux.desktop \
  "$deb_root/usr/share/applications/io.github.nahime0.ReelMux.desktop"
install -Dm644 data/io.github.nahime0.ReelMux.svg \
  "$deb_root/usr/share/icons/hicolor/scalable/apps/io.github.nahime0.ReelMux.svg"
install -Dm644 LICENSE "$deb_root/usr/share/doc/reelmux/copyright"
install -Dm644 README.md "$deb_root/usr/share/doc/reelmux/README.md"
install -Dm644 THIRD_PARTY.md "$deb_root/usr/share/doc/reelmux/THIRD_PARTY.md"
mkdir -p "$deb_root/DEBIAN"
cat > "$deb_root/DEBIAN/control" <<EOF
Package: reelmux
Version: $version
Section: video
Priority: optional
Architecture: $arch
Maintainer: ReelMux contributors <nahime0@users.noreply.github.com>
Depends: libc6, libgcc-s1, libgtk-4-1 (>= 4.10), ffmpeg
Description: MP4 track and metadata editor for Linux
 ReelMux edits tracks, subtitles, metadata and artwork in MP4 files.
EOF

dpkg-deb --root-owner-group --build "$deb_root" "dist/${base}.deb" >/dev/null
echo "dist/${base}.deb"
