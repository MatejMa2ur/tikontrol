#!/bin/sh
# Disposable CHR (Cloud Hosted Router) for testing writes, never the live router.
# Boots RouterOS arm64 in QEMU and forwards its services to localhost:
#   ssh 2222 · api 8728 · api-ssl 8729 · www 8080 · www-ssl 8443->8444
# ether1 is the WAN (QEMU user NAT), ether2-5 are isolated LAN ports, like a hAP. src/scripts/chr-defconf.rsc gives it
# a home-router style config (bridge, DHCP, firewall, WAN/LAN lists) like MikroTik's default configuration.
# A second VM (e.g. a CMR server): CHR_DIR=~/.cache/tikontrol-cmr CHR_OFFSET=100 src/scripts/chr.sh 7.26beta5
# shifts every forwarded port by CHR_OFFSET (ssh 2322, api-ssl 8829, ...). Extra forwards go in CHR_FWD, e.g.
# CHR_FWD=,hostfwd=tcp:127.0.0.1:54321-:54321 for CMR: the other VM reaches it at 10.0.2.2:54321.
# CHR_LAN adds options to ether2 (a LAN bridge port), e.g. to reach the router from its LAN side as in switch mode:
# CHR_LAN=,net=192.168.88.0/24,host=192.168.88.254,hostfwd=tcp:127.0.0.1:18729-192.168.88.1:8729,hostfwd=tcp:127.0.0.1:12222-192.168.88.1:22
# Usage: src/scripts/chr.sh [version]   (fresh disk each run: delete $DIR/disk.img to reset)
# Needs: qemu (brew install qemu, or apt install qemu-system-x86). On an arm64 Mac it boots the arm64 CHR under TCG
# emulation (with HVF the CHR kernel panics: "No working init found"). On an x86_64 host (or CHR_ARCH=x86) it boots
# the x86 CHR, with KVM when /dev/kvm is usable and TCG otherwise (about a minute to boot).
set -eu
VER=${1:-$(curl -fsS https://upgrade.mikrotik.com/routeros/NEWESTa7.stable | cut -d' ' -f1)}
DIR=${CHR_DIR:-$HOME/.cache/tikontrol-chr}
O=${CHR_OFFSET:-0}
mkdir -p "$DIR" && cd "$DIR"
ARCH=${CHR_ARCH:-$( [ "$(uname -m)" = x86_64 ] && echo x86 || echo arm64 )}
if [ "$ARCH" = x86 ]; then img=chr-$VER.img; else img=chr-$VER-arm64.img; fi
[ -f "$img" ] || { [ -f "$img.zip" ] || curl -fsSLO "https://download.mikrotik.com/routeros/$VER/$img.zip"; unzip -oq "$img.zip"; }
[ -f disk.img ] || cp "$img" disk.img
if [ "$ARCH" = x86 ]; then
  accel=tcg; [ -w /dev/kvm ] && accel=kvm
  set -- qemu-system-x86_64 -accel $accel -smp 2 -m 512 -nographic -drive file=disk.img,format=raw,if=virtio
else
  fw=$(dirname "$(command -v qemu-system-aarch64)")/../share/qemu/edk2-aarch64-code.fd
  set -- qemu-system-aarch64 -M virt -accel tcg -cpu cortex-a72 -m 512 -nographic -bios "$fw" -drive file=disk.img,format=raw,if=virtio
fi
exec "$@" \
  -netdev user,id=n0,hostfwd=tcp:127.0.0.1:$((2222 + O))-:22,hostfwd=tcp:127.0.0.1:$((8728 + O))-:8728,hostfwd=tcp:127.0.0.1:$((8729 + O))-:8729,hostfwd=tcp:127.0.0.1:$((8080 + O))-:80,hostfwd=tcp:127.0.0.1:$((8444 + O))-:443${CHR_FWD:-} \
  -device virtio-net-pci,netdev=n0 \
  -netdev user,id=n1,restrict=on${CHR_LAN:-} -device virtio-net-pci,netdev=n1 \
  -netdev user,id=n2,restrict=on -device virtio-net-pci,netdev=n2 \
  -netdev user,id=n3,restrict=on -device virtio-net-pci,netdev=n3 \
  -netdev user,id=n4,restrict=on -device virtio-net-pci,netdev=n4
