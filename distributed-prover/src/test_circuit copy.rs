use ark_ff::PrimeField;
use ark_r1cs_std::fields::FieldVar;
use ark_r1cs_std::select::CondSelectGadget;
use ark_r1cs_std::uint8::UInt8;
use ark_r1cs_std::{ToBitsGadget, ToBytesGadget};
use ark_relations::r1cs::{
    ConstraintSystem,ConstraintSystemRef, SynthesisError,
};
use std::time::Instant;
use ark_relations::ns;
use ark_r1cs_std::{
    alloc::AllocVar,
    fields::fp::FpVar,
    boolean::Boolean,
};
use ark_serialize::{CanonicalSerialize,CanonicalDeserialize};
use ark_std::{
    rand::Rng,
    vec::Vec,
    format,
};
use crate::{
    portal_manager::{PortalManager, RomProverPortalManager,SetupRomPortalManager},
    transcript::{MemType, TranscriptEntry},
    CircuitWithPortals,
};
use core::cmp::Ordering;
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::R1CSVar;  

#[derive(Copy, Clone, CanonicalSerialize, CanonicalDeserialize)]
pub struct ZkDbSqlCircuitParams {
    pub num_rows: usize,
}

impl std::fmt::Display for ZkDbSqlCircuitParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ZkDbSqlCircuitParams {{ num_rows: {} }}", self.num_rows)
    }
}

/*
select
    l_returnflag,
    l_linestatus,
    sum(l_quantity) as sum_qty,
    sum(l_extendedprice) as sum_base_price,
    sum(l_extendedprice * (1 - l_discount)) as sum_disc_price,
    sum(l_extendedprice * (1 - l_discount) * (1 + l_tax)) as sum_charge,
    avg(l_quantity) as avg_qty,
    avg(l_extendedprice) as avg_price,
    avg(l_discount) as avg_disc,
    count(*) as count_order
from
    lineitem
where
    l_shipdate <= date '1998-12-01' - interval '90' day
group by
    l_returnflag,
    l_linestatus
order by
    l_returnflag,
    l_linestatus;
*/


/// 双调排序网络 (Bitonic Sort Network) 的 gadget
struct BitonicSortGadget<F: PrimeField> {
    pub input: Vec<Vec<FpVar<F>>>,
    pub output: Vec<Vec<FpVar<F>>>,
    pub sort_key_indices: Vec<usize>, // 指定用于排序的列索引
}

impl<F: PrimeField> BitonicSortGadget<F> {
    pub fn construct(
        cs: ConstraintSystemRef<F>, 
        input: &[Vec<FpVar<F>>], 
        sort_key_indices: &[usize]
    ) -> Result<Self, SynthesisError> {
        let n = input.len();
        
        let next_pow2 = n.next_power_of_two();
        let mut padded_input = input.to_vec();
        
        // 填充行（使用零值填充）
        if n < next_pow2 {
            let row_len = if input.is_empty() { 0 } else { input[0].len() };
            for _ in n..next_pow2 {
                let mut row = Vec::with_capacity(row_len);
                for _ in 0..row_len {
                    row.push(FpVar::constant(F::zero()));
                }
                padded_input.push(row);
            }
        }
        
        // 如果长度为1，直接返回
        if next_pow2 <= 1 {
            return Ok(Self {
                input: input.to_vec(),
                output: input.to_vec(),
                sort_key_indices: sort_key_indices.to_vec(),
            });
        }
        
        let mut current = padded_input;
        
        // 实现双调排序
        for k in 1..=next_pow2.trailing_zeros() {
            let k_pow = 1 << k;
            for j in (0..k).rev() {
                let j_pow = 1 << j;
                for i in 0..next_pow2 {
                    let l = i ^ j_pow;
                    if l > i && (i & k_pow) == 0 {
                        // 升序排序
                        let cmp_result = Self::compare_rows(cs.clone(), &current[i], &current[l], sort_key_indices)?;
                        for col in 0..current[i].len() {
                            let tmp_i = current[i][col].clone();
                            let tmp_l = current[l][col].clone();
                            current[i][col] = FpVar::conditionally_select(&cmp_result, &tmp_i, &tmp_l)?;
                            current[l][col] = FpVar::conditionally_select(&cmp_result, &tmp_l, &tmp_i)?;
                        }
                    }
                    if l > i && (i & k_pow) != 0 {
                        // 降序排序
                        let cmp_result = Self::compare_rows(cs.clone(), &current[l], &current[i], sort_key_indices)?;
                        for col in 0..current[i].len() {
                            let tmp_i = current[i][col].clone();
                            let tmp_l = current[l][col].clone();
                            current[i][col] = FpVar::conditionally_select(&cmp_result, &tmp_i, &tmp_l)?;
                            current[l][col] = FpVar::conditionally_select(&cmp_result, &tmp_l, &tmp_i)?;
                        }
                    }
                }
            }
        }
        
        // 移除填充行，只保留原始长度的输出
        let output = current[0..n].to_vec();
        
        Ok(Self {
            input: input.to_vec(),
            output,
            sort_key_indices: sort_key_indices.to_vec(),
        })
    }
    
    // 比较两行，根据多个排序键
    fn compare_rows(
        cs: ConstraintSystemRef<F>,
        row_a: &[FpVar<F>],
        row_b: &[FpVar<F>],
        sort_key_indices: &[usize]
    ) -> Result<Boolean<F>, SynthesisError> {
        // 初始化比较结果为 "相等"
        let mut is_equal = Boolean::constant(true);
        let mut final_result = Boolean::constant(false);
        
        // 依次比较每个排序键
        for &idx in sort_key_indices {
            let a_lt_b = row_a[idx].is_cmp(&row_b[idx], Ordering::Less, false)?;
            let a_eq_b = row_a[idx].is_eq(&row_b[idx])?;
            
            // 如果前面的键相等，并且当前键 a < b，则 a < b
            let current_lt = Boolean::and(&is_equal, &a_lt_b)?;
            // 更新最终结果: 如果在任何排序键上 a < b，则结果为 a < b
            final_result = Boolean::or(&final_result, &current_lt)?;
            
            // 更新相等标志，继续到下一个键
            is_equal = Boolean::and(&is_equal, &a_eq_b)?;
        }
        
        Ok(final_result)
    }
}

/// 验证排序性质的 gadget
struct SortedCheckGadget<F: PrimeField> {
    pub values: Vec<Vec<FpVar<F>>>,
    pub sort_key_indices: Vec<usize>, // 指定用于排序的列索引
}

impl<F: PrimeField> SortedCheckGadget<F> {
    pub fn construct(values: &[Vec<FpVar<F>>], sort_key_indices: &[usize]) -> Self {
        Self {
            values: values.to_vec(),
            sort_key_indices: sort_key_indices.to_vec(),
        }
    }
    
    fn less_or_equal_rows(
        row_a: &[FpVar<F>],
        row_b: &[FpVar<F>],
        sort_key_indices: &[usize],
    ) -> Result<Boolean<F>, SynthesisError> {
        // 是否一直相等
        let mut is_equal = Boolean::constant(true);
        // 是否已经确定 a < b（后续列无需再看）
        let mut is_less = Boolean::constant(false);

        for &idx in sort_key_indices {
            // a < b ?
            let a_lt_b = row_a[idx].is_cmp(&row_b[idx], Ordering::Less, false)?;
            // a == b ?
            let a_eq_b = row_a[idx].is_eq(&row_b[idx])?;

            // 只有在之前所有 key 都相等时，这个列才有机会决定 a < b
            let this_key_lt = Boolean::and(&is_equal, &a_lt_b)?;
            // 如果任意列已经判断出 a < b，那就可以记下来
            is_less = Boolean::or(&is_less, &this_key_lt)?;

            // 如果本列相等，才继续让下一列去比较；否则就不再考虑后续列
            is_equal = Boolean::and(&is_equal, &a_eq_b)?;
        }

        // a <= b 当且仅当：(a < b) or (所有列都相等)
        let a_le_b = Boolean::or(&is_less, &is_equal)?;
        Ok(a_le_b)
    }
    
    pub fn generate_constraints(&self, cs: ConstraintSystemRef<F>) -> Result<(), SynthesisError> {
        let n = self.values.len();
        
        for i in 0..n-1 {
            // 判断第 i 行 <= 第 i+1 行（按多列排序）
            let a_le_b = Self::less_or_equal_rows(
                &self.values[i],
                &self.values[i+1],
                &self.sort_key_indices
            )?;
            // 强制 a_le_b == true
            a_le_b.enforce_equal(&Boolean::constant(true))?;
        }

        Ok(())

    }
}


#[derive(Clone)]
pub struct ZkDbSqlCircuit<F: PrimeField> {
    /// 从 lineitem 表读入的数据，比如 7 列：quantity, extendedprice, discount, tax, returnflag, linestatus, shipdate
    pub lineitem_table: Vec<Vec<F>>,
    /// 过滤比较的右侧 shipdate
    pub right_shipdate: F,
    pub params: ZkDbSqlCircuitParams,
}

impl<F: PrimeField> ZkDbSqlCircuit<F> {
    /// 新建一个空电路
    pub fn new(params: &ZkDbSqlCircuitParams) -> Self {
        Self {
            lineitem_table: vec![vec![F::zero(); 7]; params.num_rows],
            // lineitem_table: vec![F::one(); params.num_rows],
            right_shipdate: F::zero(),
            params: params.clone(),
        }
    }

    /// **生成随机电路**，可用于测试
    pub fn rand<R: Rng>(mut rng: R, params: &ZkDbSqlCircuitParams) -> Self {
        // 随机生成 lineitem_table
        let mut lineitem_table = Vec::with_capacity(params.num_rows);
        for _ in 0..params.num_rows {
            let row = vec![
                F::rand(&mut rng), // quantity
                F::rand(&mut rng), // extendedprice
                F::rand(&mut rng), // discount
                F::rand(&mut rng), // tax
                F::rand(&mut rng), // returnflag
                F::rand(&mut rng), // linestatus
                F::rand(&mut rng), // shipdate
            ];
            lineitem_table.push(row);
        }
        let right_shipdate = F::from(18u64);

        Self {
            lineitem_table,
            right_shipdate,
            params: params.clone(),
        }
    }
}


impl<F: PrimeField> CircuitWithPortals<F> for ZkDbSqlCircuit<F> {
    type Parameters = ZkDbSqlCircuitParams;
    const MEM_TYPE: MemType = MemType::Rom;
    type ProverPortalManager = RomProverPortalManager<F>;

    /// 子电路总数量（一个最简单的做法：每行 1 个 subcircuit，最后再加一个 padding）
    fn num_subcircuits(&self) -> usize {
        2
    }

    /// 返回一个“最小子电路集合”的索引，用于 CRS 生成等，这里 demo 返回几行
    fn get_unique_subcircuits(&self) -> Vec<usize> {
        let n = self.num_subcircuits();
        if n == 1 {
            return vec![0];
        }
        vec![0, 1]
    }

    /// 将任意 subcircuit_idx 映射为 get_unique_subcircuits 里的代表索引
    fn representative_subcircuit(&self, subcircuit_idx: usize) -> usize {
        if subcircuit_idx == 0 {
            0
        } else {
            1
        }
    }

    /// 返回电路参数
    fn get_params(&self) -> Self::Parameters {
        self.params.clone()
    }

    /// 构造一个随机电路，用于测试
    fn rand(rng: &mut impl Rng, params: &Self::Parameters) -> Self {
        Self::rand(rng, params)
    }

    /// 构造一个空电路
    fn new(params: &Self::Parameters) -> Self {
        Self::new(params)
    }


    fn get_serialized_witnesses(&self, subcircuit_idx: usize) -> Vec<u8> {
        let mut out_buf = Vec::new();
        if subcircuit_idx == 0 {
            // 序列化 right_shipdate
            // self.right_shipdate.serialize_uncompressed(&mut out_buf).unwrap();
            
            // 序列化整个表的所有字段
            for row in &self.lineitem_table {
                for &field in row {
                    field.serialize_uncompressed(&mut out_buf).unwrap();
                }
            }
        }
        out_buf
    }

    fn set_serialized_witnesses(&mut self, subcircuit_idx: usize, bytes: &[u8]) {
        if subcircuit_idx == 0 {
            
            self.lineitem_table.clear();
            for _ in 0..self.params.num_rows {
                let mut row = vec![F::zero(); 7];
                for j in 0..7 {
                    row[j] = F::deserialize_uncompressed_unchecked(&*bytes).unwrap();
                }
                self.lineitem_table.push(row);
            }

        }
    }

    /// **核心**：在 Ark R1CS 下实现电路的约束逻辑。node.rs 会在多进程中，对每个 subcircuit_idx 调用一次。
    fn generate_constraints<P: PortalManager<F>>(
        &mut self,
        cs: ConstraintSystemRef<F>,
        subcircuit_idx: usize,
        pm: &mut P,
    ) -> Result<(), SynthesisError> {
        let starting_num_constraints = cs.num_constraints();

        if subcircuit_idx < self.params.num_rows  {
            let start = Instant::now();
            let rhs_var: FpVar<F> = FpVar::new_constant(ns!(cs, "rhs_var"),  &self.right_shipdate)?;
            let lhs_var = FpVar::new_witness(ns!(cs, "lhs_var"), || Ok(self.lineitem_table[subcircuit_idx][6]))?;
            let is_less_than_var: Boolean<F> = lhs_var.is_cmp(&rhs_var, Ordering::Less, true)?;
            let one_var = FpVar::<F>::constant(F::one());
            let zero_var = FpVar::<F>::constant(F::zero());
            let cmp_res = is_less_than_var.select(&one_var, &zero_var)?;
            pm.set(format!("lhs {subcircuit_idx} com_res"), &cmp_res)?;

            let elapsed = start.elapsed();
            println!("circuit 0 Elapsed time: {:?}", elapsed);
        }else {
            let start = Instant::now();
            let mut filter_vec: Vec<FpVar<F>> = vec![];
            for (i, _) in self.lineitem_table.iter().enumerate(){
                filter_vec.push(pm.get(&format!("lhs {i} com_res")).unwrap());
            }
            let mut l_rows: Vec<Vec<FpVar<F>>> = Vec::new();
            for (row, val) in self.lineitem_table.iter_mut().zip(filter_vec.iter()) {
                let mut current_row: Vec<FpVar<F>> = Vec::new();
                for v in row {
                    current_row.push(FpVar::new_witness(ns!(cs, "lhs_var"), || Ok(v.clone()))?);
                }
                current_row.push(val.clone());
                l_rows.push(current_row);
            }

            // 定义排序键：returnflag(4), linestatus(5)
            let sort_key_indices = vec![4, 5];

            // 使用双调排序网络进行排序
            let sort_gadget = BitonicSortGadget::construct(cs.clone(), &l_rows, &sort_key_indices)?;
            let sorted_rows = sort_gadget.output;
            
            // 验证排序结果确实是有序的
            let sorted_check = SortedCheckGadget::construct(&sorted_rows, &sort_key_indices);
            sorted_check.generate_constraints(cs.clone())?;
            
            
            // let mut group_start = 0;
            // while group_start < l_sorted.len() {
            //     let mut group_end = group_start + 1;
            //     while group_end < l_sorted.len()
            //         && l_sorted[group_end][4].is_eq(&l_sorted[group_start][4]).unwrap().value().unwrap_or(false) //比较 returnflag
            //         && l_sorted[group_end][5].is_eq(&l_sorted[group_start][5]).unwrap().value().unwrap_or(false) //比较 linestatus
            //     {
            //         group_end += 1;
            //     }

            //     // 当前分组范围：[group_start, group_end)
            //     let mut sum_qty = FpVar::zero();
            //     let mut sum_base_price = FpVar::zero();
            //     let mut sum_disc_price = FpVar::zero();
            //     let mut sum_charge = FpVar::zero();
            //     let mut count_order = FpVar::zero();

            //     for i in group_start..group_end {
            //         // 仅当 shipdate 符合条件时才进行累加 (filter_vec 中的值，即 l_sorted[i][7])
            //         let filter_cond = &l_sorted[i][7]; // 获取 shipdate 比较结果

            //         // sum_qty += l_quantity * filter_cond
            //         sum_qty = sum_qty + &l_sorted[i][0] * filter_cond;

            //         // sum_base_price += l_extendedprice * filter_cond
            //         sum_base_price = sum_base_price + &l_sorted[i][1] * filter_cond;

            //         // sum_disc_price += (l_extendedprice * (1 - l_discount)) * filter_cond
            //         let one_minus_discount = FpVar::one() - &l_sorted[i][2];
            //         sum_disc_price =
            //             sum_disc_price + (&l_sorted[i][1] * &one_minus_discount) * filter_cond;

            //         // sum_charge += (l_extendedprice * (1 - l_discount) * (1 + l_tax)) * filter_cond
            //         let one_plus_tax = FpVar::one() + &l_sorted[i][3];
            //         sum_charge = sum_charge
            //             + (&l_sorted[i][1] * &one_minus_discount * &one_plus_tax) * filter_cond;
                    
            //         // count_order += filter_cond (只有符合条件的才计数)
            //         count_order = count_order + filter_cond;
            //     }

            //     // 平均值计算 (约束 sum = avg * count)
            //     // avg_qty
            //     let avg_qty = sum_qty.clone().mul_by_inverse(&count_order.clone())?;
            //     //pm.set(format!("avg_qty"), &avg_qty);
            //     // avg_price
            //     let avg_price = sum_base_price.clone().mul_by_inverse(&count_order.clone())?;
            //     //pm.set(format!("avg_price"), &avg_price);
            //     // avg_disc
            //     let avg_disc = sum_disc_price.clone().mul_by_inverse(&count_order.clone())?;
            //     //pm.set(format!("avg_disc"), &avg_disc);
            //     // 设置输出
            //     // pm.set(format!("sum_qty_{}_{}", group_start, group_end), &sum_qty)?;
            //     // pm.set(format!("sum_base_price_{}_{}", group_start, group_end), &sum_base_price)?;
            //     // pm.set(format!("sum_disc_price_{}_{}", group_start, group_end), &sum_disc_price)?;
            //     // pm.set(format!("sum_charge_{}_{}", group_start, group_end), &sum_charge)?;
            //     // pm.set(format!("avg_qty_{}_{}", group_start, group_end), &avg_qty)?;
            //     // pm.set(format!("avg_price_{}_{}", group_start, group_end), &avg_price)?;
            //     // pm.set(format!("avg_disc_{}_{}", group_start, group_end), &avg_disc)?;
            //     // pm.set(format!("count_order_{}_{}", group_start, group_end), &count_order)?;

            //     group_start = group_end;
            // }
            let elapsed = start.elapsed();
            println!("circuit 1 Elapsed time: {:?}", elapsed);
        }


        
        let ending_num_constraints = cs.num_constraints();
        println!(
            "Test subcircuit {subcircuit_idx} costs {} constraints",
            ending_num_constraints - starting_num_constraints
        );

        Ok(())
    }


    fn get_portal_subtraces(&self) -> Vec<Vec<crate::transcript::TranscriptEntry<F>>> {
        
        let cs = ConstraintSystem::new_ref();
        let mut pm = SetupRomPortalManager::new(cs.clone());
        
        // 首先为第一个子电路生成访问记录
        pm.start_subtrace(cs.clone());
        for (i, lhs_row) in self.lineitem_table.iter().enumerate() {
            // 计算并存储比较结果
            let is_less_or_equal = lhs_row[6] <= self.right_shipdate;
            let cmp_res_value = if is_less_or_equal { F::one() } else { F::zero() };
            let cmp_res = FpVar::new_witness(cs.clone(), || Ok(cmp_res_value)).unwrap();
            let _ = pm.set(format!("lhs {i} com_res"), &cmp_res);
        }

        // 然后为第二个子电路生成访问记录
        pm.start_subtrace(cs.clone());
        let mut l_sorted = Vec::with_capacity(self.lineitem_table.len());
        for (i, _) in self.lineitem_table.iter().enumerate() {
            // 从 pm 中读出比较结果
            let cmp_res_var = pm.get(&format!("lhs {i} com_res")).unwrap();
            let cmp_res_value = cmp_res_var.value().unwrap_or(F::zero());

            // 把原本的 7 个字段 + 比较结果 合并
            let mut row_extended = self.lineitem_table[i].clone();
            row_extended.push(cmp_res_value);
            l_sorted.push(row_extended);
        }

        // // 2) 按照 [4], [5] 列分别表示的 returnflag, linestatus 排序
        // //    generate_constraints 里是用 `is_cmp` 处理，但这里是原生处理
        // l_sorted.sort_by(|a, b| {
        //     // a[4],a[5] 对应 returnflag, linestatus
        //     // a[0],a[1],a[2],a[3] 分别对应 quantity, extendedprice, discount, tax
        //     let a_rf = a[4];
        //     let b_rf = b[4];
        //     if a_rf < b_rf {
        //         return Ordering::Less;
        //     } else if a_rf > b_rf {
        //         return Ordering::Greater;
        //     }
        //     let a_ls = a[5];
        //     let b_ls = b[5];
        //     if a_ls < b_ls {
        //         Ordering::Less
        //     } else if a_ls > b_ls {
        //         Ordering::Greater
        //     } else {
        //         Ordering::Equal
        //     }
        // });

        // // 3) 分组聚合
        // //    逐组计算 sum_qty, sum_base_price, sum_disc_price, sum_charge, count_order 等
        // let mut group_start = 0;
        // while group_start < l_sorted.len() {
        //     let mut group_end = group_start + 1;
        //     // 同一分组：判断 returnflag, linestatus 是否相等
        //     while group_end < l_sorted.len()
        //         && l_sorted[group_end][4] == l_sorted[group_start][4]
        //         && l_sorted[group_end][5] == l_sorted[group_start][5]
        //     {
        //         group_end += 1;
        //     }

        //     // 计算分组内的指标
        //     let mut sum_qty = F::zero();
        //     let mut sum_base_price = F::zero();
        //     let mut sum_disc_price = F::zero();
        //     let mut sum_charge = F::zero();
        //     let mut count_order = F::zero();

        //     // 当前分组范围：[group_start, group_end)
        //     for i in group_start..group_end {
        //         let filter_cond = l_sorted[i][7]; // cmp_res
        //         // sum_qty += l_quantity * filter_cond
        //         sum_qty += l_sorted[i][0] * filter_cond;
        //         // sum_base_price += l_extendedprice * filter_cond
        //         sum_base_price += l_sorted[i][1] * filter_cond;
        //         // sum_disc_price += (l_extendedprice * (1 - l_discount)) * filter_cond
        //         let one_minus_discount = F::one() - l_sorted[i][2];
        //         sum_disc_price += l_sorted[i][1] * one_minus_discount * filter_cond;
        //         // sum_charge += (l_extendedprice * (1 - l_discount) * (1 + l_tax)) * filter_cond
        //         let one_plus_tax = F::one() + l_sorted[i][3];
        //         sum_charge += l_sorted[i][1] * one_minus_discount * one_plus_tax * filter_cond;
        //         // count_order += filter_cond
        //         count_order += filter_cond;
        //     }

        //     // 计算平均数
        //     let avg_qty = if count_order.is_zero() {
        //         F::zero()
        //     } else {
        //         sum_qty * count_order.inverse().unwrap()
        //     };
        //     let avg_price = if count_order.is_zero() {
        //         F::zero()
        //     } else {
        //         sum_base_price * count_order.inverse().unwrap()
        //     };
        //     let avg_disc = if count_order.is_zero() {
        //         F::zero()
        //     } else {
        //         sum_disc_price * count_order.inverse().unwrap()
        //     };

        //     // 4) 把结果写回 pm（与 generate_constraints 保持一致）
        //     let sum_qty_var = FpVar::new_witness(cs.clone(), || Ok(sum_qty)).unwrap();
        //     pm.set(format!("sum_qty_{}_{}", group_start, group_end), &sum_qty_var).unwrap();

        //     let sum_base_price_var = FpVar::new_witness(cs.clone(), || Ok(sum_base_price)).unwrap();
        //     pm.set(
        //         format!("sum_base_price_{}_{}", group_start, group_end),
        //         &sum_base_price_var,
        //     )
        //     .unwrap();

        //     let sum_disc_price_var =
        //         FpVar::new_witness(cs.clone(), || Ok(sum_disc_price)).unwrap();
        //     pm.set(
        //         format!("sum_disc_price_{}_{}", group_start, group_end),
        //         &sum_disc_price_var,
        //     )
        //     .unwrap();

        //     let sum_charge_var = FpVar::new_witness(cs.clone(), || Ok(sum_charge)).unwrap();
        //     pm.set(format!("sum_charge_{}_{}", group_start, group_end), &sum_charge_var)
        //         .unwrap();

        //     let avg_qty_var = FpVar::new_witness(cs.clone(), || Ok(avg_qty)).unwrap();
        //     pm.set(format!("avg_qty_{}_{}", group_start, group_end), &avg_qty_var)
        //         .unwrap();

        //     let avg_price_var = FpVar::new_witness(cs.clone(), || Ok(avg_price)).unwrap();
        //     pm.set(format!("avg_price_{}_{}", group_start, group_end), &avg_price_var)
        //         .unwrap();

        //     let avg_disc_var = FpVar::new_witness(cs.clone(), || Ok(avg_disc)).unwrap();
        //     pm.set(format!("avg_disc_{}_{}", group_start, group_end), &avg_disc_var)
        //         .unwrap();

        //     let count_order_var =
        //         FpVar::new_witness(cs.clone(), || Ok(count_order)).unwrap();
        //     pm.set(
        //         format!("count_order_{}_{}", group_start, group_end),
        //         &count_order_var,
        //     )
        //     .unwrap();

        //     // 移动到下一个分组
        //     group_start = group_end;
        // }
        
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