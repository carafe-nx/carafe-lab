#!/bin/sh
set -eu

src=/src/mesa
build=/src/build
out=/out
mesa=/lab/external/mesa
patches=/lab/patches/mesa
mkdir -p "$out"
git config --global --add safe.directory '*'

rev=$(git -C "$mesa" rev-parse HEAD)
stamp="$rev $(cat "$patches"/*.patch 2>/dev/null | sha256sum | cut -d' ' -f1)"
if [ "$(cat "$src/.carafe-patches" 2>/dev/null)" != "$stamp" ]; then
    previous="$src.previous"
    rm -rf "$previous"
    if [ -d "$src/.git" ]; then
        git -C "$src" diff --name-only | while read -r file; do
            mkdir -p "$previous/$(dirname "$file")"
            cp -p "$src/$file" "$previous/$file"
        done
    else
        git clone -q --no-checkout "$mesa" "$src"
    fi
    if ! git -C "$src" cat-file -e "$rev^{commit}" 2>/dev/null; then
        git -C "$src" fetch -q "$mesa" "$rev"
    fi
    git -C "$src" checkout -q --force --detach "$rev"
    git -C "$src" reset -q --hard "$rev"
    git -C "$src" clean -q -fd
    for patch in "$patches"/*.patch; do
        [ -e "$patch" ] || continue
        echo "patch: $(basename "$patch")"
        git -C "$src" apply --whitespace=nowarn "$patch"
    done
    if [ -d "$previous" ]; then
        (cd "$previous" && find . -type f) | while read -r file; do
            if cmp -s "$previous/$file" "$src/$file"; then
                touch -r "$previous/$file" "$src/$file"
            fi
        done
        rm -rf "$previous"
    fi
    echo "$stamp" > "$src/.carafe-patches"
fi

if [ ! -f "$build/build.ninja" ]; then
    meson setup "$build" "$src" --prefix=/opt/mesa-d3d12 --libdir=lib \
        -Dbuildtype=debugoptimized -Db_ndebug=true \
        -Dplatforms=x11 -Dgallium-drivers=d3d12 -Dvulkan-drivers= \
        -Dglx=dri -Degl=enabled -Dgbm=disabled -Dgles1=disabled -Dgles2=enabled -Dglvnd=enabled \
        -Dllvm=disabled -Dgallium-va=disabled -Dgallium-vdpau=disabled -Dgallium-rusticl=false \
        -Dmicrosoft-clc=disabled -Dvalgrind=disabled -Dlibunwind=disabled -Dlmsensors=disabled \
        >"$out/configure.log" 2>&1
fi
jobs="${CARAFE_JOBS:-$(nproc)}"
ninja -C "$build" -j"$jobs" >"$out/build.log" 2>&1
rm -rf "$out/opt"
DESTDIR="$out" meson install -C "$build" --no-rebuild >>"$out/build.log" 2>&1
mkdir -p "$out/opt/carafe/lib"
gcc -O2 -Wall -fPIC -shared -o "$out/opt/carafe/lib/libcarafe-sigchain.so" /tools/carafe-sigchain.c
echo "mesa built from $rev with $(ls "$patches"/*.patch 2>/dev/null | wc -l) patches"
