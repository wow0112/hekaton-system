use ark_ff::PrimeField;
use ark_relations::r1cs::{
    ConstraintSystem, ConstraintSystemRef, SynthesisError,
};

use std::time::Instant;
use ark_relations::ns;
use ark_r1cs_std::{
    alloc::AllocVar,
    fields::fp::FpVar,
    boolean::Boolean,
    bits::uint64::UInt64,
};
use ark_serialize::{CanonicalSerialize, CanonicalDeserialize};
use ark_std::{
    rand::Rng,
    vec::Vec,
    format,
};
use crate::{
    portal_manager::{PortalManager, RomProverPortalManager, SetupRomPortalManager}, 
    subcircuit_circuit, transcript::{MemType, TranscriptEntry}, CircuitWithPortals
};
use core::cmp::Ordering;
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::R1CSVar;

#[derive(Copy, Clone, CanonicalSerialize, CanonicalDeserialize)]
pub struct ZkDbSqlCircuitParams {
    pub num_rows: usize,
    pub sort_column_idx: usize,  // 需要排序的列索引
}

impl std::fmt::Display for ZkDbSqlCircuitParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ZkDbSqlCircuitParams {{ num_rows: {}, sort_column_idx: {} }}", 
               self.num_rows, self.sort_column_idx)
    }
}

#[derive(Clone)]
pub struct ZkDbSqlCircuit<F: PrimeField> {
    pub table_data: Vec<Vec<F>>,
    /// 排序后的索引
    pub sorted_indices: Vec<usize>,
    pub params: ZkDbSqlCircuitParams,
}

impl<F: PrimeField> CircuitWithPortals<F> for ZkDbSqlCircuit<F> {
    type Parameters = ZkDbSqlCircuitParams;
    const MEM_TYPE: MemType = MemType::Rom;
    type ProverPortalManager = RomProverPortalManager<F>;

    /// 计算归并排序需要的层数，即 log₂(n) 向上取整。每一层作为一个子电路
    fn num_subcircuits(&self) -> usize {
        // 使用二分归并排序，处理log2(n)层
        let mut size = self.params.num_rows;
        let mut count = 0;
        while size > 1 {
            size = (size + 1) / 2;
            count += 1;
        }
        count
    }

    fn get_unique_subcircuits(&self) -> Vec<usize> {
        // 每一层都具有唯一性
        (0..self.num_subcircuits()).collect()
    }

    fn representative_subcircuit(&self, subcircuit_idx: usize) -> usize {
        subcircuit_idx
    }

    fn get_params(&self) -> ZkDbSqlCircuitParams {
        self.params
    }

    fn rand(rng: &mut impl Rng, &params: &ZkDbSqlCircuitParams) -> Self {
        Self::new(&params)
    }

    fn new(&params: &Self::Parameters) -> Self {
        // 初始化表格数据和排序索引
        let table_data = vec![vec![F::from(12u64); 7]; params.num_rows];
        let sorted_indices = (0..params.num_rows).collect();
        
        ZkDbSqlCircuit {
            table_data,
            sorted_indices,
            params,
        }
    }

    fn get_serialized_witnesses(&self, subcircuit_idx: usize) -> Vec<u8> {
        let mut out_buf = Vec::new();
        
        // 计算在这一层中有多少对要归并
        let total_layers = self.num_subcircuits();
        let current_layer = total_layers - subcircuit_idx - 1;
        let segment_size = 2usize.pow(current_layer as u32);
        let num_segments = (self.params.num_rows + segment_size - 1) / segment_size;
        let num_pairs = (num_segments) / 2;
        
        // 对于每一对要归并的段，序列化排序前的索引和对应的值
        for pair_idx in 0..num_pairs {
            let left_start = pair_idx * 2 * segment_size;
            let right_start = left_start + segment_size;
            let right_end = std::cmp::min(right_start + segment_size, self.params.num_rows);
            
            // 序列化左段长度和右段长度
            let left_len = segment_size;
            let right_len = right_end - right_start;
            
            
            // 序列化左段tabledata和索引
            for i in 0..left_len {
                if left_start + i < self.params.num_rows {
                    let idx = self.sorted_indices[left_start + i];
                    let sort_key = self.table_data[idx][self.params.sort_column_idx];
                    
                    sort_key.serialize_uncompressed(&mut out_buf).unwrap();
                    F::from(idx as u64).serialize_uncompressed(&mut out_buf).unwrap();
                }
            }
            
            // 序列化右段tabledata和索引
            for i in 0..right_len {
                if right_start + i < self.params.num_rows {
                    let idx = self.sorted_indices[right_start + i];
                    let sort_key = self.table_data[idx][self.params.sort_column_idx];
                    
                    sort_key.serialize_uncompressed(&mut out_buf).unwrap();
                    F::from(idx as u64).serialize_uncompressed(&mut out_buf).unwrap();
                }
            }
        }
        
        out_buf
    }

    fn set_serialized_witnesses(&mut self, subcircuit_idx: usize, bytes: &[u8]) {
        let field_size = F::one().uncompressed_size();
        let mut offset = 0;
        
        // 计算在这一层中有多少对要归并
        let total_layers = self.num_subcircuits();
        let current_layer = total_layers - subcircuit_idx - 1;
        let segment_size = 2usize.pow(current_layer as u32);
        let num_segments = (self.params.num_rows + segment_size - 1) / segment_size;
        let num_pairs = (num_segments) / 2;
        
        for pair_idx in 0..num_pairs {
            let left_start = pair_idx * 2 * segment_size;
            let right_start = left_start + segment_size;
            let right_end = std::cmp::min(right_start + segment_size, self.params.num_rows);
            let output_start = left_start;
            
            // 直接计算左段长度和右段长度，不再从序列化数据中读取
            let left_len = segment_size;
            let right_len = right_end - right_start;
            
            // 跳过左段和右段的排序键和索引
            let entries_to_skip = (left_len + right_len) * 2;
            offset += entries_to_skip * field_size;
            
            let field_size = usize::.uncompressed_size();
            // 反序列化归并后的索引
            let merged_len = left_len + right_len;
            for i in 0..merged_len {
                if output_start + i < self.params.num_rows {
                    let sorted_idx = usize::deserialize_uncompressed_unchecked(&bytes[offset..(offset + field_size)]).unwrap();
                    offset += field_size;
                    
                    self.sorted_indices[output_start + i] = sorted_idx;
                }
            }
        }
    }

    fn generate_constraints<P: PortalManager<F>>(
        &mut self,
        cs: ConstraintSystemRef<F>,
        subcircuit_idx: usize,
        pm: &mut P,
    ) -> Result<(), SynthesisError> {
        let starting_num_constraints = cs.num_constraints();
        
        // 计算在这一层中有多少对要归并
        let total_layers = self.num_subcircuits();
        let current_layer = total_layers - subcircuit_idx - 1;
        // 当前层每个段的大小
        let segment_size = 2usize.pow(current_layer as u32);
        // 总共有多少个段
        let num_segments = (self.params.num_rows + segment_size - 1) / segment_size;
        // 需要归并的段对数量
        let num_pairs = (num_segments) / 2;
        
        // 对于每一对要归并的段，生成约束
        for pair_idx in 0..num_pairs {
            // 计算归并的段的边界
            let left_start = pair_idx * 2 * segment_size;
            let right_start = left_start + segment_size;
            let right_end = std::cmp::min(right_start + segment_size, self.params.num_rows);
            let output_start = left_start;
            
            
            // 创建存储左段和右段的数组
            let mut left_keys = Vec::with_capacity(segment_size);
            let mut left_indices = Vec::with_capacity(segment_size);
            let mut right_keys = Vec::with_capacity(right_end - right_start);
            let mut right_indices = Vec::with_capacity(right_end - right_start);
            
            // 为左段分配变量
            for i in 0..segment_size {
                if left_start + i < self.params.num_rows {
                    let idx = self.sorted_indices[left_start + i];
                    let sort_key = self.table_data[idx][self.params.sort_column_idx];
                    
                    let key_var = FpVar::new_witness(
                        ns!(cs, "left_key_{pair_idx}_{i}"), 
                        || Ok(sort_key)
                    )?;
                    
                    let idx_var = FpVar::new_witness(
                        ns!(cs, "left_idx_{pair_idx}_{i}"), 
                        || Ok(F::from(idx as u64))
                    )?;
                    
                    left_keys.push(key_var);
                    left_indices.push(idx_var);
                }
            }
            
            // 为右段分配变量
            for i in 0..(right_end - right_start) {
                if right_start + i < self.params.num_rows {
                    let idx = self.sorted_indices[right_start + i];
                    let sort_key = self.table_data[idx][self.params.sort_column_idx];
                    
                    let key_var = FpVar::new_witness(
                        ns!(cs, "right_key_{pair_idx}_{i}"), 
                        || Ok(sort_key)
                    )?;
                    
                    let idx_var = FpVar::new_witness(
                        ns!(cs, "right_idx_{pair_idx}_{i}"), 
                        || Ok(F::from(idx as u64))
                    )?;
                    
                    right_keys.push(key_var);
                    right_indices.push(idx_var);
                }
            }
            
            // 执行归并过程并生成约束
            let mut merged_indices = Vec::new();
            let mut left_ptr = 0;
            let mut right_ptr = 0;
            
            // 实际的归并过程
            while left_ptr < left_keys.len() && right_ptr < right_keys.len() {
                // 比较左右两个键的大小
                let is_left_smaller = left_keys[left_ptr].is_cmp(&right_keys[right_ptr], Ordering::Less, false)?;
                
                // 根据比较结果选择左边或右边的索引
                let selected_idx = is_left_smaller.select(&left_indices[left_ptr], &right_indices[right_ptr])?;
                merged_indices.push(selected_idx);
                
                // 更新指针
                if is_left_smaller.value().unwrap() {
                    left_ptr += 1;
                } else {
                    right_ptr += 1;
                }

            }
            
            // 处理剩余元素
            while left_ptr < left_keys.len() {
                merged_indices.push(left_indices[left_ptr].clone());
                left_ptr += 1;
            }
            
            while right_ptr < right_keys.len() {
                merged_indices.push(right_indices[right_ptr].clone());
                right_ptr += 1;
            }
            
            // 将合并后的索引写入portal
            for (i, idx) in merged_indices.iter().enumerate() {
                pm.set(format!("merged_indices_{pair_idx}_{i}"), idx)?;
            }
        }
        
        let ending_num_constraints = cs.num_constraints();
        println!(
            "Sort subcircuit {subcircuit_idx} costs {} constraints",
            ending_num_constraints - starting_num_constraints
        );
        
        Ok(())
    }

    fn get_portal_subtraces(&self) -> Vec<Vec<crate::transcript::TranscriptEntry<F>>> {
        let cs = ConstraintSystem::new_ref();
        let mut pm = SetupRomPortalManager::new(cs.clone());
        
        // 计算总层数
        let total_layers = self.num_subcircuits();
        
        // 为每一层创建subtrace
        for layer_idx in 0..total_layers {
            pm.start_subtrace(ConstraintSystem::new_ref());
            
            let current_layer = total_layers - layer_idx - 1;
            let segment_size = 2usize.pow(current_layer as u32);
            let num_segments = (self.params.num_rows + segment_size - 1) / segment_size;
            let num_pairs = (num_segments) / 2;
            
            // 对于每一对要归并的段
            for pair_idx in 0..num_pairs {
                let output_start = pair_idx * 2 * segment_size;
                let left_start = output_start;
                let right_start = left_start + segment_size;
                let right_end = std::cmp::min(right_start + segment_size, self.params.num_rows);
                
                // 执行归并排序
                let mut left = Vec::new();
                let mut right = Vec::new();
                
                // 准备左段和右段数据
                for i in 0..segment_size {
                    if left_start + i < self.params.num_rows {
                        let idx = self.sorted_indices[left_start + i];
                        left.push((self.table_data[idx][self.params.sort_column_idx], idx));
                    }
                }
                
                for i in 0..(right_end - right_start) {
                    if right_start + i < self.params.num_rows {
                        let idx = self.sorted_indices[right_start + i];
                        right.push((self.table_data[idx][self.params.sort_column_idx], idx));
                    }
                }
                
                // 执行归并
                let mut merged = Vec::new();
                let mut i = 0;
                let mut j = 0;
                
                while i < left.len() && j < right.len() {
                    if left[i].0 <= right[j].0 {
                        merged.push(left[i]);
                        i += 1;
                    } else {
                        merged.push(right[j]);
                        j += 1;
                    }
                }
                
                // 处理剩余元素
                while i < left.len() {
                    merged.push(left[i]);
                    i += 1;
                }
                
                while j < right.len() {
                    merged.push(right[j]);
                    j += 1;
                }
                
                // 将归并结果写入portal
                for (i, (_, idx)) in merged.iter().enumerate() {
                    let idx_var = FpVar::new_witness(cs.clone(), || Ok(F::from(*idx as u64))).unwrap();
                    let _ = pm.set(format!("merged_indices_{pair_idx}_{i}"), &idx_var);
                }
            }
        }
        
        // 包装和返回访问记录
        pm.subtraces
            .into_iter()
            .map(|subtrace| {
                subtrace
                    .into_iter()
                    .map(|e| TranscriptEntry::Rom(e))
                    .collect()
            })
            .collect()
    }
}

// // 辅助函数：基于索引进行归并排序
// fn merge_sort<F: PrimeField>(circuit: &mut ZkDbSqlCircuit<F>) {
//     let num_rows = circuit.params.num_rows;
//     let col_idx = circuit.params.sort_column_idx;
    
//     // 初始化索引数组
//     let mut indices: Vec<usize> = (0..num_rows).collect();
    
//     // 执行归并排序
//     merge_sort_recursive(&circuit.table_data, &mut indices, 0, num_rows - 1, col_idx);
    
//     // 更新排序后的索引
//     circuit.sorted_indices = indices;
// }

// fn merge_sort_recursive<F: PrimeField>(
//     data: &Vec<Vec<F>>, 
//     indices: &mut Vec<usize>, 
//     start: usize, 
//     end: usize, 
//     col_idx: usize
// ) {
//     if start < end {
//         let mid = start + (end - start) / 2;
        
//         // 递归排序左半部分和右半部分
//         merge_sort_recursive(data, indices, start, mid, col_idx);
//         merge_sort_recursive(data, indices, mid + 1, end, col_idx);
        
//         // 合并两个已排序的半部分
//         merge(data, indices, start, mid, end, col_idx);
//     }
// }

// fn merge<F: PrimeField>(
//     data: &Vec<Vec<F>>, 
//     indices: &mut Vec<usize>, 
//     start: usize, 
//     mid: usize, 
//     end: usize, 
//     col_idx: usize
// ) {
//     let n1 = mid - start + 1;
//     let n2 = end - mid;
    
//     // 创建临时数组
//     let mut left_indices = Vec::with_capacity(n1);
//     let mut right_indices = Vec::with_capacity(n2);
    
//     // 复制数据到临时数组
//     for i in 0..n1 {
//         left_indices.push(indices[start + i]);
//     }
    
//     for i in 0..n2 {
//         right_indices.push(indices[mid + 1 + i]);
//     }
    
//     // 合并临时数组
//     let mut i = 0;
//     let mut j = 0;
//     let mut k = start;
    
//     while i < n1 && j < n2 {
//         // 比较排序键
//         if data[left_indices[i]][col_idx] <= data[right_indices[j]][col_idx] {
//             indices[k] = left_indices[i];
//             i += 1;
//         } else {
//             indices[k] = right_indices[j];
//             j += 1;
//         }
//         k += 1;
//     }
    
//     // 复制剩余元素
//     while i < n1 {
//         indices[k] = left_indices[i];
//         i += 1;
//         k += 1;
//     }
    
//     while j < n2 {
//         indices[k] = right_indices[j];
//         j += 1;
//         k += 1;
//     }
// }