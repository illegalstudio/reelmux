#!/usr/bin/env bash
# Package one ReelMux Linux binary as a tarball and native packages.

set -euo pipefail

binary="${1:?usage: package.sh <binary> <version> <arch>}"
version="${2:?usage: package.sh <binary> <version> <arch>}"
arch="${3:?usage: package.sh <binary> <version> <arch>}"

if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
  echo "error: version must use MAJOR.MINOR.PATCH (got '$version')" >&2
  exit 1
fi

case "$arch" in
  amd64)
    archlinux_arch="x86_64"
    ;;
  arm64)
    archlinux_arch="aarch64"
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

mkdir -p dist
base="reelmux_${version}_linux_${arch}"
stage="$(mktemp -d)"
trap 'rm -rf "$stage" dist/.stage' EXIT

archive_root="$stage/$base"
install -Dm755 "$binary" "$archive_root/bin/reelmux"
install -Dm644 data/io.github.nahime0.ReelMux.desktop \
  "$archive_root/share/applications/io.github.nahime0.ReelMux.desktop"
install -Dm644 data/io.github.nahime0.ReelMux.svg \
  "$archive_root/share/icons/hicolor/scalable/apps/io.github.nahime0.ReelMux.svg"
install -Dm644 data/io.github.nahime0.ReelMux.metainfo.xml \
  "$archive_root/share/metainfo/io.github.nahime0.ReelMux.metainfo.xml"
install -Dm644 LICENSE "$archive_root/LICENSE"
install -Dm644 README.md "$archive_root/README.md"
install -Dm644 THIRD_PARTY.md "$archive_root/THIRD_PARTY.md"

tar -C "$stage" -czf "dist/${base}.tar.gz" "$base"
echo "dist/${base}.tar.gz"

NFPM="${NFPM:-$(command -v nfpm || true)}"
if [[ -z "$NFPM" ]]; then
  echo "nfpm not found; skipping deb, rpm and Arch Linux packages" >&2
  exit 0
fi

mkdir -p dist/.stage
install -Dm755 "$binary" dist/.stage/reelmux
export VERSION="$version" ARCH="$arch"
for format in deb rpm archlinux; do
  case "$format" in
    deb) output="dist/${base}.deb" ;;
    rpm) output="dist/${base}.rpm" ;;
    archlinux) output="dist/reelmux-${version}-1-${archlinux_arch}.pkg.tar.zst" ;;
  esac
  "$NFPM" package \
    --config packaging/nfpm.yaml \
    --packager "$format" \
    --target "$output" >/dev/null
  echo "$output"
done
