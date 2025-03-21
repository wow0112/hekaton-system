#!/bin/bash

rm -rf co-test/* pk-test/*  req/* resp-s0/* resp-s1/*

cargo build --bin coordinator
cargo build --bin worker

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/coordinator  gen-keys --g16-pk-dir ./pk-nc=4-ns=4-np=4  --coord-state-dir ./co-nc=4-ns=4-np=4 --num-subcircuits 4 --num-sha2-iters 4 --num-portals 4
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "gen-keys elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/coordinator start-stage0  --req-dir ./req  --coord-state-dir ./co-nc=4-ns=4-np=4
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "start-stage0 elapsed time: ${elapsed} seconds"


# 记录开始时间（单位秒）
start_time=$(date +%s)
echo " Running 4096 stage0 workers in parallel..."
MAX_CONCURRENT=100
for i in $(seq 0 3); do
        /home/wh/hekaton-system/target/debug/worker process-stage0-request --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-nc=4-ns=4-np=4 --req-dir /home/wh/hekaton-system/distributed-prover/param/req --out-dir /home/wh/hekaton-system/distributed-prover/param/resp --subcircuit-index "$i" &
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
/home/wh/hekaton-system/target/debug/coordinator start-stage1 --req-dir ./req  --coord-state-dir ./co-nc=4-ns=4-np=4 --resp-dir ./resp
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "start-stage1 elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
MAX_CONCURRENT=100
for i in $(seq 0 3); do
        /home/wh/hekaton-system/target/debug/worker process-stage1-request --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-nc=4-ns=4-np=4  --req-dir /home/wh/hekaton-system/distributed-prover/param/req --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp  --out-dir /home/wh/hekaton-system/distributed-prover/param/resp --subcircuit-index "$i" &
	if (( $(jobs -r | wc -l) >= MAX_CONCURRENT )); then
                wait -n  # 等待至少一个后台进程结束，再启动新进程
        fi
done
wait
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "process-stage1-request  elapsed time: ${elapsed} seconds"

start_time=$(date +%s)
/home/wh/hekaton-system/target/debug/coordinator end-proof --coord-state-dir ./co-nc=4-ns=4-np=4 --resp-dir ./resp
end_time=$(date +%s)
elapsed=$((end_time - start_time))
echo "process-stage1-request  elapsed time: ${elapsed} seconds"
