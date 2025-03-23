#!/bin/bash

# 检查是否提供了参数
if [ $# -ne 1 ]; then
    echo "Usage: $0 <num_rows>"
    echo "Example: $0 4096"
    exit 1
fi

# 从命令行参数获取 NUM_ROWS
NUM_ROWS=$1
NUM_SUBCIRCUITS=$(( (NUM_ROWS + 63) / 64 ))

CPU_CORES=$(nproc)
MAX_CONCURRENT=$(( CPU_CORES ))

rm -rf co-test/* pk-test/* req/* resp/*

cargo build --bin test_co --release --features parallel
cargo build --bin test-worker --release --features parallel
echo "子电路数量 (NUM_SUBCIRCUITS=$NUM_SUBCIRCUITS)"
# 创建计时文件
TIMING_FILE="time_num_rows=${NUM_ROWS}.txt"
echo "分布式证明系统计时结果 (NUM_ROWS=$NUM_ROWS)" > $TIMING_FILE
echo "开始时间: $(date)" >> $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE

TOTAL_START_TIME=$(date +%s)

# gen-keys 计时
echo "正在运行 gen-keys..." | tee -a $TIMING_FILE
GEN_KEYS_START=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co  gen-keys --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-test  --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test --num-rows $NUM_ROWS
GEN_KEYS_END=$(date +%s)
GEN_KEYS_TIME=$((GEN_KEYS_END - GEN_KEYS_START))
echo "gen-keys 完成，耗时 ${GEN_KEYS_TIME} 秒" | tee -a $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE

# start-stage0 计时
echo "正在运行 start-stage0..." | tee -a $TIMING_FILE
STAGE0_START=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co start-stage0 --req-dir /home/wh/hekaton-system/distributed-prover/param/req  --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test
STAGE0_END=$(date +%s)
STAGE0_TIME=$((STAGE0_END - STAGE0_START))
echo "start-stage0 完成，耗时 ${STAGE0_TIME} 秒" | tee -a $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE

# process-stage0-request 计时
echo "正在运行 process-stage0-request..." | tee -a $TIMING_FILE
PROCESS_STAGE0_TOTAL_START=$(date +%s)

for i in $(seq 0 $((NUM_SUBCIRCUITS-1))); do
        /home/wh/hekaton-system/target/debug/test-worker process-stage0-request --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-test --req-dir /home/wh/hekaton-system/distributed-prover/param/req --out-dir /home/wh/hekaton-system/distributed-prover/param/resp --subcircuit-index "$i" &
        if (( $(jobs -r | wc -l) >= MAX_CONCURRENT )); then
                wait -n  # 等待至少一个后台进程结束，再启动新进程
        fi
done
wait

PROCESS_STAGE0_TOTAL_END=$(date +%s)
PROCESS_STAGE0_TOTAL_TIME=$((PROCESS_STAGE0_TOTAL_END - PROCESS_STAGE0_TOTAL_START))
echo "所有 process-stage0-request 完成，总耗时 ${PROCESS_STAGE0_TOTAL_TIME} 秒" | tee -a $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE

# start-stage1 计时
echo "正在运行 start-stage1..." | tee -a $TIMING_FILE
STAGE1_START=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co start-stage1 --req-dir /home/wh/hekaton-system/distributed-prover/param/req  --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp
STAGE1_END=$(date +%s)
STAGE1_TIME=$((STAGE1_END - STAGE1_START))
echo "start-stage1 完成，耗时 ${STAGE1_TIME} 秒" | tee -a $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE

# process-stage1-request 计时
echo "正在运行 process-stage1-request..." | tee -a $TIMING_FILE
PROCESS_STAGE1_TOTAL_START=$(date +%s)

for i in $(seq 0 $((NUM_SUBCIRCUITS-1))); do
        /home/wh/hekaton-system/target/debug/test-worker process-stage1-request --g16-pk-dir /home/wh/hekaton-system/distributed-prover/param/pk-test --req-dir /home/wh/hekaton-system/distributed-prover/param/req --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp  --out-dir /home/wh/hekaton-system/distributed-prover/param/resp --subcircuit-index "$i" &
        if (( $(jobs -r | wc -l) >= MAX_CONCURRENT )); then
                wait -n  # 等待至少一个后台进程结束，再启动新进程
        fi
done
wait

PROCESS_STAGE1_TOTAL_END=$(date +%s)
PROCESS_STAGE1_TOTAL_TIME=$((PROCESS_STAGE1_TOTAL_END - PROCESS_STAGE1_TOTAL_START))
echo "所有 process-stage1-request 完成，总耗时 ${PROCESS_STAGE1_TOTAL_TIME} 秒" | tee -a $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE


export RAYON_NUM_THREADS=$CPU_CORES

# end-proof 计时
echo "正在运行 end-proof..." | tee -a $TIMING_FILE
END_PROOF_START=$(date +%s)
/home/wh/hekaton-system/target/debug/test_co end-proof --coord-state-dir /home/wh/hekaton-system/distributed-prover/param/co-test --resp-dir /home/wh/hekaton-system/distributed-prover/param/resp
END_PROOF_END=$(date +%s)
END_PROOF_TIME=$((END_PROOF_END - END_PROOF_START))
echo "end-proof 完成，耗时 ${END_PROOF_TIME} 秒" | tee -a $TIMING_FILE
echo "----------------------------------------" >> $TIMING_FILE


# 计算总时间
TOTAL_END_TIME=$(date +%s)
TOTAL_TIME=$((TOTAL_END_TIME - TOTAL_START_TIME))

echo "总结:" | tee -a $TIMING_FILE
echo "gen-keys: ${GEN_KEYS_TIME} 秒" | tee -a $TIMING_FILE
echo "start-stage0: ${STAGE0_TIME} 秒" | tee -a $TIMING_FILE
echo "所有 process-stage0-request: ${PROCESS_STAGE0_TOTAL_TIME} 秒" | tee -a $TIMING_FILE
echo "start-stage1: ${STAGE1_TIME} 秒" | tee -a $TIMING_FILE
echo "所有 process-stage1-request: ${PROCESS_STAGE1_TOTAL_TIME} 秒" | tee -a $TIMING_FILE
echo "end-proof: ${END_PROOF_TIME} 秒" | tee -a $TIMING_FILE
echo "总执行时间: ${TOTAL_TIME} 秒" | tee -a $TIMING_FILE
echo "完成时间: $(date)" | tee -a $TIMING_FILE
