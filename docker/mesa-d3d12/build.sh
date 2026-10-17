#!/bin/sh
set -eu

MESA_VERSION=25.2.8
MESA_DEBIAN=25.2.8-0ubuntu0.24.04.4

apt-get update
apt-get install -y --no-install-recommends \
    meson-1.7 python3-mako python3-yaml python3-packaging directx-headers-dev patch \
    libdrm-dev libexpat1-dev flex bison libglvnd-dev libx11-xcb-dev libxfixes-dev \
    libxcb-glx0-dev libxcb-dri2-0-dev libxcb-dri3-dev libxcb-present-dev libxcb-shm0-dev \
    libxcb-randr0-dev libxcb-sync-dev libxcb-xfixes0-dev libxshmfence-dev libxxf86vm-dev

mkdir -p /tmp/mesa
cd /tmp/mesa
curl -fsSLO "http://archive.ubuntu.com/ubuntu/pool/main/m/mesa/mesa_${MESA_VERSION}.orig.tar.xz"
curl -fsSLO "http://archive.ubuntu.com/ubuntu/pool/main/m/mesa/mesa_${MESA_DEBIAN}.debian.tar.xz"
tar xf "mesa_${MESA_VERSION}.orig.tar.xz"
cd "mesa-${MESA_VERSION}"
tar xf "../mesa_${MESA_DEBIAN}.debian.tar.xz"
for p in $(grep -v '^#' debian/patches/series); do patch -p1 < "debian/patches/$p"; done
patch -p1 < /fix/mesa-d3d12-query-recursion.patch
meson setup build --prefix=/opt/mesa-d3d12 --libdir=lib \
    -Dbuildtype=debugoptimized -Db_ndebug=true \
    -Dplatforms=x11 -Dgallium-drivers=d3d12 -Dvulkan-drivers= \
    -Dglx=dri -Degl=enabled -Dgbm=disabled -Dgles1=disabled -Dgles2=enabled -Dglvnd=enabled \
    -Dllvm=disabled -Dgallium-va=disabled -Dgallium-vdpau=disabled -Dgallium-rusticl=false \
    -Dmicrosoft-clc=disabled -Dvalgrind=disabled -Dlibunwind=disabled -Dlmsensors=disabled
ninja -C build install

mkdir -p /opt/carafe/lib
gcc -O2 -Wall -fPIC -shared -o /opt/carafe/lib/libcarafe-sigchain.so /fix/carafe-sigchain.c
ls -la /opt/mesa-d3d12/lib /opt/carafe/lib
