#!/bin/sh
# Disposable CHR (Cloud Hosted Router) for testing writes, never the live router.
# Boots RouterOS arm64 in QEMU and forwards its services to localhost:
#   ssh 2222 · api 8728 · api-ssl 8729 · www 8080 · www-ssl 8443->8444
# Usage: src/scripts/chr.sh [version]   (fresh disk each run: delete $DIR/disk.img to reset)
# Needs: qemu (brew install qemu). Runs under TCG emulation: with HVF on Apple Silicon the CHR kernel panics ("No working init found").
set -eu
VER=${1:-$(curl -fsS https://upgrade.mikrotik.com/routeros/NEWESTa7.stable | cut -d' ' -f1)}
DIR=${CHR_DIR:-$HOME/.cache/tikontrol-chr}
mkdir -p "$DIR" && cd "$DIR"
img=chr-$VER-arm64.img
[ -f "$img" ] || { curl -fsSLO "https://download.mikrotik.com/routeros/$VER/$img.zip" && unzip -oq "$img.zip"; }
[ -f disk.img ] || cp "$img" disk.img
fw=$(dirname "$(command -v qemu-system-aarch64)")/../share/qemu/edk2-aarch64-code.fd
exec qemu-system-aarch64 -M virt -accel tcg -cpu cortex-a72 -m 512 -nographic \
  -bios "$fw" -drive file=disk.img,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:2222-:22,hostfwd=tcp:127.0.0.1:8728-:8728,hostfwd=tcp:127.0.0.1:8729-:8729,hostfwd=tcp:127.0.0.1:8080-:80,hostfwd=tcp:127.0.0.1:8444-:443 \
  -device virtio-net-pci,netdev=n0
