#!/bin/sh
set -eu

gpu="${CARAFE_GPU:-wsl}"
case "$gpu" in
    wsl)
        if [ ! -e /dev/dxg ]; then
            echo "run-eden: /dev/dxg is missing; start the container with --device /dev/dxg" >&2
            exit 64
        fi
        export LD_LIBRARY_PATH="/usr/lib/wsl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        for file in /opt/mesa-d3d12/lib/libGLX_mesa.so.0 /opt/carafe/lib/libcarafe-sigchain.so; do
            if [ ! -e "$file" ]; then
                echo "run-eden: $file is missing; the image was built without carafe-lab-mesa-d3d12" >&2
                exit 65
            fi
        done
        export LD_LIBRARY_PATH="/opt/mesa-d3d12/lib:$LD_LIBRARY_PATH"
        export __GLX_VENDOR_LIBRARY_NAME=mesa
        export LD_PRELOAD="/opt/carafe/lib/libcarafe-sigchain.so${LD_PRELOAD:+:$LD_PRELOAD}"
        export GALLIUM_DRIVER=d3d12
        export MESA_D3D12_DEFAULT_ADAPTER_NAME="${CARAFE_ADAPTER:-}"
        export VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/dzn_icd*.json 2>/dev/null | head -n 1)"
        ;;
    software)
        export LIBGL_ALWAYS_SOFTWARE=1
        export MESA_GL_VERSION_OVERRIDE=4.6
        export MESA_GLSL_VERSION_OVERRIDE=460
        export MESA_VK_VERSION_OVERRIDE=1.3
        export VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -n 1)"
        ;;
    *)
        echo "run-eden: CARAFE_GPU must be wsl or software, not $gpu" >&2
        exit 64
        ;;
esac
[ -n "$VK_ICD_FILENAMES" ] || unset VK_ICD_FILENAMES

Xvfb :1 -screen 0 1280x720x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
export DISPLAY=:1
tries=0
until xdpyinfo >/dev/null 2>&1; do
    tries=$((tries + 1))
    if [ "$tries" -gt 100 ]; then
        echo "run-eden: Xvfb did not start" >&2
        cat /tmp/xvfb.log >&2
        exit 66
    fi
    sleep 0.1
done

case "${1:-}" in
    gpu-info)
        echo "== OpenGL"
        glxinfo -B 2>&1 || true
        echo "== Vulkan"
        vulkaninfo --summary 2>&1 || true
        exit 0
        ;;
    version)
        cat /opt/eden/carafe-version
        exit 0
        ;;
    "")
        echo "usage: run-eden <file.nsp> | session | gpu-info | version" >&2
        exit 64
        ;;
esac

case "${CARAFE_VIDEO:-full}" in
    full)
        x11vnc -display :1 -localhost -forever -shared -nopw -quiet -rfbport 5900 >/tmp/x11vnc.log 2>&1 &
        ;;
    low)
        x11vnc -display :1 -localhost -forever -shared -nopw -quiet -rfbport 5900 -wait 200 -defer 200 \
            -nonap >/tmp/x11vnc.log 2>&1 &
        ;;
    off)
        ;;
    *)
        echo "run-eden: CARAFE_VIDEO must be full, low or off, not $CARAFE_VIDEO" >&2
        exit 64
        ;;
esac
if [ "${CARAFE_VIDEO:-full}" != off ]; then
    websockify --web /usr/share/novnc 6080 127.0.0.1:5900 >/tmp/websockify.log 2>&1 &
fi

mkdir -p /work/user/log
(
    while :; do
        printf '%s %s %s\n' "$(date +%s%3N)" \
            "$(awk '$1 == "anon" || $1 == "shmem" { sum += $2 } END { print sum }' /sys/fs/cgroup/memory.stat)" \
            "$(sed -n 's/^usage_usec //p' /sys/fs/cgroup/cpu.stat)"
        sleep 1
    done >>/work/user/log/resources.log 2>/dev/null
) &
cd /work
eden=/opt/eden/bin/eden
[ -x /opt/eden/AppRun ] && eden=/opt/eden/AppRun
if [ "$1" = session ]; then
    exec "$eden" -f
fi
exec "$eden" -f -g "$1"
