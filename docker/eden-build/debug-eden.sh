#!/bin/sh
set -eu

nsp="$1"
out=/out/cli
bin=/src/build-debug/bin

if [ "${CARAFE_GPU:-software}" = wsl ]; then
    export LD_LIBRARY_PATH="/usr/lib/wsl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    export GALLIUM_DRIVER=d3d12
    renderer="${CARAFE_RENDERER:-0}"
else
    export LIBGL_ALWAYS_SOFTWARE=1
    export MESA_VK_VERSION_OVERRIDE=1.3
    VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/lvp_icd*.json | head -n 1)"
    export VK_ICD_FILENAMES
    renderer=1
fi

mkdir -p /tmp/fb
Xvfb :1 -screen 0 1280x720x24 -fbdir /tmp/fb -nolisten tcp >/tmp/xvfb.log 2>&1 &
export DISPLAY=:1
sleep 1

ln -sfn /work/user "$bin/user"
cat > /tmp/cli.ini <<'EOF'
[Core]
use_multi_core\default=false
use_multi_core=true

[Cpu]
cpu_backend\default=false
cpu_backend=0

[Renderer]
backend\default=false
backend=1

[Audio]
output_engine\default=false
output_engine=3

[Miscellaneous]
log_filter\default=false
log_filter=*:Info Kernel.SVC:Trace HW.GPU:Debug Service.NVDRV:Debug
EOF
sed -i "s/^backend=1\$/backend=$renderer/" /tmp/cli.ini
if [ "${CARAFE_GPU:-software}" = wsl ]; then
    sed -i 's/^cpu_backend=0$/cpu_backend=0\ncpu_accuracy\\default=false\ncpu_accuracy=4\ncpuopt_fastmem\\default=false\ncpuopt_fastmem=false/' /tmp/cli.ini
fi

cat > /tmp/gdb.cmd <<'EOF'
set pagination off
set print thread-events off
handle SIGSEGV nostop noprint pass
handle SIGBUS nostop noprint pass
handle SIGUSR1 nostop noprint pass
handle SIGUSR2 nostop noprint pass
handle SIGPIPE nostop noprint pass
break AssertFailSoftImpl if $_any_caller_matches("Jit", 4)
source /tmp/gdb-extra.cmd
run
echo \n=== stopped ===\n
x/12i $pc-24
info registers x0 x1 x2 x3 x16 x17 x30 sp pc
bt 40
echo \n=== all threads ===\n
info threads
thread apply all bt 25
kill
quit
EOF

if [ -f /tools/gdb-extra.cmd ]; then
    cp /tools/gdb-extra.cmd /tmp/gdb-extra.cmd
else
    : > /tmp/gdb-extra.cmd
fi

ulimit -s "${CARAFE_STACK_KB:-8192}"

cd "$bin"
if [ "${CARAFE_GDB:-1}" = 0 ]; then
    screens="$out/screens"
    rm -rf "$screens"
    mkdir -p "$screens"
    "$bin/eden-cli" --config=/tmp/cli.ini -g "$nsp" >"$out/eden-cli.log" 2>&1 &
    eden=$!
    seconds=0
    while kill -0 "$eden" 2>/dev/null && [ "$seconds" -lt "${CARAFE_SECONDS:-300}" ]; do
        sleep 30
        seconds=$((seconds + 30))
        python3 /tools/xwd2png.py /tmp/fb/Xvfb_screen0 "$screens/$(printf '%04d' "$seconds").png" || true
    done
    kill "$eden" 2>/dev/null || true
    wait "$eden" 2>/dev/null || true
    echo "eden-cli ran $seconds s; screens in $screens"
else
    gdb -q -batch -x /tmp/gdb.cmd --args "$bin/eden-cli" --config=/tmp/cli.ini -g "$nsp" >"$out/gdb.log" 2>&1 || true
    tail -n 60 "$out/gdb.log"
fi
cp /work/user/log/eden_log.txt "$out/eden_log.txt" 2>/dev/null || true
