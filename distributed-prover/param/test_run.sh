#!/bin/bash

# 检查是否提供了参数
if [ $# -ne 1 ]; then
    echo "Usage: $0 <num_rows>"
    echo "Example: $0 4096"
    exit 1
fi

# 从命令行参数获取 NUM_ROWS
NUM_ROWS=$1

rm -rf co-test/* pk-test/* req/* resp/*

cargo build --bin test_co
cargo build --bin test-worker

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co  gen-keys --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-test  --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test --num-rows $NUM_ROWS
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "gen-keys elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co start-stage0 --req-dir /home/wh/hekaton-system/distributed-prover/param/req  --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "start-stage0 elapsed time: ${elapsed} seconds"

# 记录开始时间（单位秒）
start_time=$(date +%s)
echo " Running $NUM_ROWS stage0 workers in parallel..."
MAX_CONCURRENT=100
for i in $(seq 0 $((NUM_ROWS-1))); do
        /home/wh/hekaton-system/target/debug/test-worker process-stage0-request --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-test --req-dir /home/wh/hekaton-system/distributed-prover/param/req --out-dir /home/wh/hekaton-system/distributed-prover/param/resp --subcircuit-index "$i" &
        if (( $(jobs -r | wc -l) >= MAX_CONCURRENT )); then
                wait -n  # 等待至少一个后台进程结束，再启动新进程
        fi
done
wait
# 记录结束时间，并计算总耗时
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "process-stage0-request  elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co start-stage1 --req-dir /home/wh/hekaton-system/distributed-prover/param/req  --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "start-stage1 elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
MAX_CONCURRENT=100
for i in $(seq 0 $((NUM_ROWS-1))); do
        /home/wh/hekaton-system/target/debug/test-worker process-stage1-request --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-test --req-dir /home/wh/hekaton-system/distributed-prover/param/req --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp  --out-dir /home/wh/hekaton-system/distributed-prover/param/resp --subcircuit-index "$i" &
        if (( $(jobs -r | wc -l) >= MAX_CONCURRENT )); then
                wait -n  # 等待至少一个后台进程结束，再启动新进程
        fi
done
wait
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "process-stage1-request  elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co end-proof --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "end-proof  elapsed time: ${elapsed} seconds"
