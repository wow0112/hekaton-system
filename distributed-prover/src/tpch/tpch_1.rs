// use ark_ff::PrimeField;
// use ark_relations::r1cs::{
//     ConstraintSystemRef, Namespace, SynthesisError,
// };
// use ark_r1cs_std::{
//     alloc::AllocVar,
//     eq::EqGadget,
//     fields::fp::FpVar,
//     boolean::Boolean,
// };
// use ark_serialize::{CanonicalSerialize,CanonicalDeserialize};
// use ark_std::{
//     rand::Rng,
//     vec::Vec,
//     format,
// };
// use crate::{
//     portal_manager::{PortalManager, RomProverPortalManager, SetupRomPortalManager},
//     transcript::MemType,
//     CircuitWithPortals,
// };


// #[derive(Copy, Clone, CanonicalSerialize, CanonicalDeserialize)]
// pub struct ZkDbSqlCircuitParams {
//     pub num_rows: usize,
//     // 你可以根据需要增加更多字段，例如 groupby 字段数、阈值设定、是否做多列筛选 等
// }

// impl std::fmt::Display for ZkDbSqlCircuitParams {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "ZkDbSqlCircuitParams {{ num_rows: {} }}", self.num_rows)
//     }
// }


// #[derive(Clone)]
// pub struct ZkDbSqlCircuit<F: PrimeField> {
//     /// 从 lineitem 表读入的数据，比如 7 列：quantity, extendedprice, discount, tax, returnflag, linestatus, shipdate
//     pub lineitem_table: Vec<[F; 7]>,
//     /// 过滤比较的右侧 shipdate
//     pub right_shipdate: F,
//     /// 记录下电路参数
//     pub params: ZkDbSqlCircuitParams,
// }

// impl<F: PrimeField> ZkDbSqlCircuit<F> {
//     /// 新建一个空电路
//     pub fn new(params: &ZkDbSqlCircuitParams) -> Self {
//         Self {
//             lineitem_table: vec![[F::zero(); 7]; params.num_rows],
//             right_shipdate: F::zero(),
//             params: params.clone(),
//         }
//     }

//     /// **生成随机电路**，可用于测试
//     pub fn rand<R: Rng>(mut rng: R, params: &ZkDbSqlCircuitParams) -> Self {
//         // 随机生成 lineitem_table
//         let mut lineitem_table = Vec::with_capacity(params.num_rows);
//         for _ in 0..params.num_rows {
//             let row = [
//                 F::rand(&mut rng), // quantity
//                 F::rand(&mut rng), // extendedprice
//                 F::rand(&mut rng), // discount
//                 F::rand(&mut rng), // tax
//                 F::rand(&mut rng), // returnflag
//                 F::rand(&mut rng), // linestatus
//                 F::rand(&mut rng), // shipdate
//             ];
//             lineitem_table.push(row);
//         }
//         let right_shipdate = F::rand(&mut rng);

//         Self {
//             lineitem_table,
//             right_shipdate,
//             params: params.clone(),
//         }
//     }
// }

// /// 为了让 node.rs 能够并行、分配子电路计算，需要实现 CircuitWithPortals
// impl<F: PrimeField> CircuitWithPortals<F> for ZkDbSqlCircuit<F> {
//     type Parameters = ZkDbSqlCircuitParams;
//     const MEM_TYPE: MemType = MemType::Rom;
//     type ProverPortalManager = RomProverPortalManager<F>;

//     /// 子电路总数量（一个最简单的做法：每行 1 个 subcircuit，最后再加一个 padding）
//     fn num_subcircuits(&self) -> usize {
//         self.params.num_rows + 1  // +1 作为占位padding 
//     }

//     /// 返回一个“最小子电路集合”的索引，用于 CRS 生成等，这里 demo 返回几行
//     fn get_unique_subcircuits(&self) -> Vec<usize> {
//         // 例如第一行、第二行、最后一行、以及 padding
//         let n = self.num_subcircuits();
//         if n == 1 {
//             return vec![0];
//         }
//         vec![0, 1, n - 1]
//     }

//     /// 将任意 subcircuit_idx 映射为 get_unique_subcircuits 里的代表索引
//     fn representative_subcircuit(&self, subcircuit_idx: usize) -> usize {
//         let n = self.num_subcircuits();
//         if subcircuit_idx == 0 {
//             0
//         } else if subcircuit_idx == n - 1 {
//             n - 1
//         } else {
//             // demo：除开首尾，其余都映射到 1
//             1
//         }
//     }

//     /// 返回电路参数
//     fn get_params(&self) -> Self::Parameters {
//         self.params.clone()
//     }

//     /// 构造一个随机电路，用于测试
//     fn rand(rng: &mut impl Rng, params: &Self::Parameters) -> Self {
//         Self::rand(rng, params)
//     }

//     /// 构造一个空电路
//     fn new(params: &Self::Parameters) -> Self {
//         Self::new(params)
//     }

//     /// 读取某个 subcircuit 的所有 witness 并序列化输出。  
//     /// 类似参考 Merkle 代码中那样，这里可以根据 subcircuit 的含义来决定拿哪些字段做 witness。
//     fn get_serialized_witnesses(&self, subcircuit_idx: usize) -> Vec<u8> {
//         let num_rows = self.params.num_rows;
//         let total_subcircuits = self.num_subcircuits();

//         // padding subcircuit 不返回
//         if subcircuit_idx == total_subcircuits - 1 {
//             return vec![];
//         }

//         // demo：将第 i 行转为 bytes
//         // 如果 subcircuit_idx >= num_rows，也就是 padding，多余行都不返回
//         if subcircuit_idx < num_rows {
//             // 这里随意用 Ark 自身序列化，或者 bincode、postcard 等
//             let row = &self.lineitem_table[subcircuit_idx];
//             let mut buf = Vec::new();
//             // row[7], plus 1shipdate
//             // 这里只是演示把 [F; 7] 转成 bytes
//             for &field in row.iter() {
//                 field.serialize_compressed(&mut buf).unwrap();
//             }

//             let mut right_ship_buf = Vec::new();
//             self.right_shipdate.serialize_compressed(&mut right_ship_buf).unwrap();

//             // 也把 right_shipdate 放进去
//             buf.extend_from_slice(&right_ship_buf);

//             buf
//         } else {
//             vec![]
//         }
//     }

//     /// 从字节反序列化某个 subcircuit 的 witness
//     fn set_serialized_witnesses(&mut self, subcircuit_idx: usize, bytes: &[u8]) {
//         let num_rows = self.params.num_rows;
//         let total_subcircuits = self.num_subcircuits();
//         // padding 跳过
//         if subcircuit_idx == total_subcircuits - 1 {
//             return;
//         }
//         if subcircuit_idx >= num_rows {
//             return;
//         }

//         // demo: 从 bytes 中依次读出 7 个 F & 1 个 right_shipdate
//         let mut cursor = bytes;
//         let mut row = [F::zero(); 7];
//         for i in 0..7 {
//             row[i] = F::deserialize_compressed_unchecked(&mut cursor).unwrap();
//         }
//         self.lineitem_table[subcircuit_idx] = row;
//         let rs = F::deserialize_compressed_unchecked(&mut cursor).unwrap();
//         self.right_shipdate = rs;
//     }

//     /// **核心**：在 Ark R1CS 下实现电路的约束逻辑。node.rs 会在多进程中，对每个 subcircuit_idx 调用一次。
//     fn generate_constraints<P: PortalManager<F>>(
//         &mut self,
//         cs: ConstraintSystemRef<F>,
//         subcircuit_idx: usize,
//         pm: &mut P,
//     ) -> Result<(), SynthesisError> {
//         let nrows = self.params.num_rows;
//         let total_subcircuits = self.num_subcircuits();

//         // 如果是 padding，什么也不做
//         if subcircuit_idx == total_subcircuits - 1 {
//             // 占位操作
//             // 也可以像 MerkleTreeCircuit 那样做 dummy hash
//             return Ok(());
//         }

//         // 取出本 subcircuit 的那一行（或几行）
//         // 这里 demo: 每个 subcircuit_idx 就是 1 行
//         if subcircuit_idx >= nrows {
//             return Ok(());
//         }

//         let row = self.lineitem_table[subcircuit_idx];
//         // row 依次: [quantity, extendedprice, discount, tax, returnflag, linestatus, shipdate]
//         let quantity_f = row[0];
//         let extendedprice_f = row[1];
//         let discount_f = row[2];
//         let tax_f = row[3];
//         let returnflag_f = row[4];
//         let linestatus_f = row[5];
//         let shipdate_f = row[6];

//         // 分配到 R1CS witness
//         let quantity_var = FpVar::new_witness(ark_relations::ns!(cs, "quantity"), || Ok(quantity_f))?;
//         let extendedprice_var = FpVar::new_witness(ark_relations::ns!(cs, "extprice"), || Ok(extendedprice_f))?;
//         let discount_var = FpVar::new_witness(ark_relations::ns!(cs, "discount"), || Ok(discount_f))?;
//         let tax_var = FpVar::new_witness(ark_relations::ns!(cs, "tax"), || Ok(tax_f))?;
//         let returnflag_var = FpVar::new_witness(ark_relations::ns!(cs, "returnflag"), || Ok(returnflag_f))?;
//         let linestatus_var = FpVar::new_witness(ark_relations::ns!(cs, "linestatus"), || Ok(linestatus_f))?;
//         let shipdate_var = FpVar::new_witness(ark_relations::ns!(cs, "shipdate"), || Ok(shipdate_f))?;

//         let right_shipdate_var = FpVar::new_witness(ark_relations::ns!(cs, "rightship"), || {
//             Ok(self.right_shipdate)
//         })?;

//         // 1) 筛选条件: shipdate <= right_shipdate ?
//         //    这里为了演示，只写了一个“shipdate <= right_shipdate -> bool” 的做法
//         //    Ark 没有内置的“<=” gadget，需要你自己实现一个 comparator gadget。
//         //    此处只演示一个“如果 shipdate > right_shipdate，就记 is_valid = 0，否则 1” 的简易做法
//         let is_valid = {
//             // 替代做法：定义一个 (shipdate - right_shipdate) >= 0 判断
//             // 如果你有一个更完善的 compare_gadget，可以直接调用
//             let diff = &shipdate_var - &right_shipdate_var;
//             // 这里我们只演示如何做一个“是否相等”的布尔约束
//             // 真正要做 <= 需要多字节/多比特分解
//             // demo: 简化写法——如果 diff 为负，则标记 is_valid = 1，否则 = 0
//             // 你需要实现 is_non_positive(diff)
//             let bool_var = Boolean::constant(false); 
//             // TODO: 使用自定义 comparator 实现 (请根据自身需求做)
//             bool_var
//         };

//         // 2) 仅当 is_valid=1 时，把 (quantity, extendedprice, discount, ...) 加到 groupby 结果
//         //    我们模拟 MerkleCircuit 中的 portal，在 pm 里存储中间结果。
//         //    在多行执行后（多 subcircuit），我们可以在“父 subcircuit”或“最终汇总 subcircuit”里把它们 sum 上去。
//         //    这里 demo：把 “quantity” 的值存到 "row_{i}_quantity" portal 里，如果 is_valid=0，那就存 0。
//         let quantity_true_var = quantity_var.clone() * is_valid.clone().into();  // 这里 is_valid.into() -> FpVar<F>
//         pm.set(format!("row_{}_quantity", subcircuit_idx), &quantity_true_var)?;

//         // 你可以将多列分别 set，也可以把它们 concat 成一个 FpVar 整理
//         // ...

//         // 3) 如果我们有“父 subcircuit”要把相邻行 returnflag 对比做排序校验
//         //    就可以在此把 returnflag_var 设到 Portal
//         pm.set(format!("row_{}_returnflag", subcircuit_idx), &returnflag_var)?;

//         // 4) 还可以做更多诸如 groupby 统计、count、discount 累加的逻辑

//         // (demo END) 
//         // 实际生产建议把Halo2 等逻辑：ls_returnflag 排序、linestatus 排序、sum_qty、sum_base_price、sum_disc_price等
//         // 在Ark中也要做相应R1CS 约束，这里限于篇幅不一一实现。

//         Ok(())
//     }

//     // 如果还需要自定义一份 Portal trace（可选）
//     fn get_portal_subtraces(&self) -> Vec<Vec<crate::transcript::TranscriptEntry<F>>> {
//         // 类似 MerkleTreeCircuit::get_portal_subtraces() 的实现
//         // 如果不需要，也可直接返回空
//         vec![]
//     }
// }