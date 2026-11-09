#!/bin/sh
# Disposable CHR (Cloud Hosted Router) for testing writes, never the live router.
# Boots RouterOS arm64 in QEMU and forwards its services to localhost:
#   ssh 2222 · api 8728 · api-ssl 8729 · www 8080 · www-ssl 8443->8444
# ether1 is the WAN (QEMU user NAT), ether2-5 are isolated LAN ports, like a hAP. src/scripts/chr-defconf.rsc gives it
# a home-router style config (bridge, DHCP, firewall, WAN/LAN lists) like MikroTik's default configuration.
# A second VM (e.g. a CMR server): CHR_DIR=~/.cache/tikontrol-cmr CHR_OFFSET=100 src/scripts/chr.sh 7.26beta5
# shifts every forwarded port by CHR_OFFSET (ssh 2322, api-ssl 8829, ...). Extra forwards go in CHR_FWD, e.g.
# CHR_FWD=,hostfwd=tcp:127.0.0.1:54321-:54321 for CMR: the other VM reaches it at 10.0.2.2:54321.
# Usage: src/scripts/chr.sh [version]   (fresh disk each run: delete $DIR/disk.img to reset)
# Needs: qemu (brew install qemu). Runs under TCG emulation: with HVF on Apple Silicon the CHR kernel panics ("No working init found").
set -eu
VER=${1:-$(curl -fsS https://upgrade.mikrotik.com/routeros/NEWESTa7.stable | cut -d' ' -f1)}
DIR=${CHR_DIR:-$HOME/.cache/tikontrol-chr}
O=${CHR_OFFSET:-0}
mkdir -p "$DIR" && cd "$DIR"
img=chr-$VER-arm64.img
[ -f "$img" ] || { [ -f "$img.zip" ] || curl -fsSLO "https://download.mikrotik.com/routeros/$VER/$img.zip"; unzip -oq "$img.zip"; }
[ -f disk.img ] || cp "$img" disk.img
fw=$(dirname "$(command -v qemu-system-aarch64)")/../share/qemu/edk2-aarch64-code.fd
exec qemu-system-aarch64 -M virt -accel tcg -cpu cortex-a72 -m 512 -nographic \
  -bios "$fw" -drive file=disk.img,format=raw,if=virtio \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:$((2222 + O))-:22,hostfwd=tcp:127.0.0.1:$((8728 + O))-:8728,hostfwd=tcp:127.0.0.1:$((8729 + O))-:8729,hostfwd=tcp:127.0.0.1:$((8080 + O))-:80,hostfwd=tcp:127.0.0.1:$((8444 + O))-:443${CHR_FWD:-} \
  -device virtio-net-pci,netdev=n0 \
  -netdev user,id=n1,restrict=on -device virtio-net-pci,netdev=n1 \
  -netdev user,id=n2,restrict=on -device virtio-net-pci,netdev=n2 \
  -netdev user,id=n3,restrict=on -device virtio-net-pci,netdev=n3 \
  -netdev user,id=n4,restrict=on -device virtio-net-pci,netdev=n4
