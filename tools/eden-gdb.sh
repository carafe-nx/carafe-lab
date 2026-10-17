#!/bin/sh
set -eu

export MSYS_NO_PATHCONV=1
container=carafe-lab-eden

case "${1:-}" in
attach)
    until docker exec "$container" grep -q "Starting server" /work/user/log/eden_log.txt 2>/dev/null; do
        sleep 1
    done
    docker exec -d "$container" sh -c '
        rm -f /tmp/g /tmp/gdb.out
        mkfifo /tmp/g
        tail -f /tmp/g | gdb -q -nx -ex "set pagination off" -ex "set confirm off" -ex "set width 0" \
            -ex "target remote 127.0.0.1:6543" -ex "continue" > /tmp/gdb.out 2>&1'
    echo "gdb attached"
    ;;
run)
    shift
    mark="--- $(date +%s) ---"
    docker exec "$container" sh -c 'kill -INT $(pgrep -x gdb)'
    sleep 3
    {
        echo "echo $mark\\n"
        for command in "$@"; do
            echo "$command"
        done
        echo "echo --- end ---\\n"
        echo "continue"
    } | docker exec -i "$container" sh -c 'cat > /tmp/g'
    until docker exec "$container" sh -c "sed -n '/$mark/,\$p' /tmp/gdb.out | grep -q -- '--- end ---'"; do
        sleep 2
    done
    docker exec "$container" sh -c "sed -n '/$mark/,/--- end ---/p' /tmp/gdb.out"
    ;;
*)
    echo "usage: eden-gdb.sh attach | run <gdb command>..." >&2
    exit 2
    ;;
esac
