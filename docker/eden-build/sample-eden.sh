#!/bin/sh
set -eu

count="${1:-20}"
gap="${2:-0.5}"
out=/work/user/log/eden-samples.txt
pid=$(pgrep -o -x eden || pgrep -o eden)
: > "$out"
i=0
while [ "$i" -lt "$count" ]; do
    echo "=== sample $i $(date +%s.%N)" >> "$out"
    gdb -q -batch -p "$pid" \
        -ex "set pagination off" \
        -ex "set print thread-events off" \
        -ex "thread apply all bt 12" >> "$out" 2>/dev/null || true
    i=$((i + 1))
    sleep "$gap"
done
echo "samples in $out"
