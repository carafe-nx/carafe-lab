set -eu
make -C /lab/tools/probes host-gl-build CXX=g++-14 OUT=/tmp/memory-policy
export LD_LIBRARY_PATH=/opt/mesa-d3d12/lib:/usr/lib/wsl/lib
export LD_PRELOAD=/opt/carafe/lib/libcarafe-sigchain.so
export __GLX_VENDOR_LIBRARY_NAME=mesa
export GALLIUM_DRIVER=d3d12
export MESA_D3D12_DEFAULT_ADAPTER_NAME="${CARAFE_ADAPTER:-}"
export DISPLAY=:97
Xvfb :97 -screen 0 1280x720x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
test_xvfb_pid=$!
trap 'kill "$test_xvfb_pid" 2>/dev/null || true' EXIT
test_attempt=0
until xdpyinfo >/dev/null 2>&1; do
    test_attempt=$((test_attempt + 1))
    if [ "$test_attempt" -ge 100 ]; then cat /tmp/xvfb.log; exit 2; fi
    sleep 0.1
done
timeout 60s /tmp/memory-policy/host/opengl-staging-test
