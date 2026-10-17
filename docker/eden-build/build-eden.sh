#!/bin/sh
set -eu

frontend="${LAB_FRONTEND:-qt}"
src=/src/eden
eden=/lab/external/eden
patches=/lab/patches/eden
case "$frontend" in
    cli)
        build=/src/build-debug
        out=/out/cli
        options="-DENABLE_QT=OFF -DYUZU_CMD=ON"
        target=eden-cli
        ;;
    qt)
        build=/src/build-qt
        out=/out/qt
        options="-DENABLE_QT=ON -DYUZU_CMD=OFF -DENABLE_QT_TRANSLATION=OFF"
        target=eden
        ;;
    *)
        echo "build-eden: LAB_FRONTEND must be cli or qt, not $frontend" >&2
        exit 64
        ;;
esac
mkdir -p "$out"
git config --global --add safe.directory '*'

rev=$(git -C "$eden" rev-parse HEAD)
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
        git clone -q --no-checkout "$eden" "$src"
    fi
    if ! git -C "$src" cat-file -e "$rev^{commit}" 2>/dev/null; then
        git -C "$src" fetch -q "$eden" "$rev"
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
    cmake -S "$src" -B "$build" -G Ninja -DCMAKE_BUILD_TYPE=RelWithDebInfo -DCPM_SOURCE_CACHE=/src/cpm-cache \
        $options -DYUZU_TESTS=OFF -DENABLE_WEB_SERVICE=OFF -DUSE_DISCORD_PRESENCE=OFF \
        -DENABLE_UPDATE_CHECKER=OFF -DYUZU_ROOM=OFF >"$out/configure.log" 2>&1
fi
jobs="${CARAFE_JOBS:-$(nproc)}"
ninja -C "$build" -j"$jobs" "$target" >"$out/build.log" 2>&1
if [ "$frontend" = qt ]; then
    cp "$build/bin/eden" "$out/eden"
fi
echo "$target built from $rev with $(ls "$patches"/*.patch 2>/dev/null | wc -l) patches"
