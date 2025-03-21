use ark_ff::PrimeField;
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
    portal_manager::{PortalManager, RomProverPortalManager,SetupRomPortalManager}, subcircuit_circuit, transcript::{MemType, TranscriptEntry}, CircuitWithPortals
};
use core::cmp::Ordering;
use ark_r1cs_std::prelude::*;
use ark_r1cs_std::R1CSVar;  

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


#[derive(Copy, Clone, CanonicalSerialize, CanonicalDeserialize)]
pub struct ZkDbSqlCircuitParams {
    pub num_rows: usize,
}

impl std::fmt::Display for ZkDbSqlCircuitParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ZkDbSqlCircuitParams {{ num_rows: {} }}", self.num_rows)
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

impl<F: PrimeField> CircuitWithPortals<F> for ZkDbSqlCircuit<F> {
    type Parameters = ZkDbSqlCircuitParams;
    const MEM_TYPE: MemType = MemType::Rom;
    type ProverPortalManager = RomProverPortalManager<F>;

    /// 子电路总数量
    fn num_subcircuits(&self) -> usize {
        self.params.num_rows
    }

    /// 返回一个“最小子电路集合”的索引
    fn get_unique_subcircuits(&self) -> Vec<usize> {
        vec![0,1,3]
    }

    /// 将任意 subcircuit_idx 映射为 get_unique_subcircuits 里的代表索引
    fn representative_subcircuit(&self, subcircuit_idx: usize) -> usize {
        if subcircuit_idx == 0{
            0
        }else if subcircuit_idx == self.params.num_rows -1 {
            3
        }else{
            1
        }
    }

    /// 返回电路参数
    fn get_params(&self) -> ZkDbSqlCircuitParams {
        self.params
    }

    /// 构造一个随机电路，用于测试
    fn rand(rng: &mut impl Rng, &params: &ZkDbSqlCircuitParams) -> Self {
        Self::new(&params)
    }

    /// 构造一个空电路
    fn new(&params: &Self::Parameters) -> Self {
        let right_shipdate = F::from(12u64);
        ZkDbSqlCircuit {
            lineitem_table: vec![vec![F::one(); 7]; params.num_rows],
            right_shipdate,
            params,
        }
    }

    fn get_serialized_witnesses(&self, subcircuit_idx: usize) -> Vec<u8> {
        let mut out_buf = Vec::new();
        if subcircuit_idx < self.params.num_rows {
            self.lineitem_table[subcircuit_idx][6].serialize_uncompressed(&mut out_buf).unwrap();
            self.right_shipdate.serialize_uncompressed(&mut out_buf).unwrap();
        }else{
            panic!("subcircuit_idx > self.params.num_rows");
        }
        out_buf
    }

    fn set_serialized_witnesses(&mut self, subcircuit_idx: usize, bytes: &[u8]) {
        if subcircuit_idx < self.params.num_rows {
            let field_size = F::one().uncompressed_size();
            self.lineitem_table[subcircuit_idx][6] = F::deserialize_uncompressed_unchecked(&bytes[..field_size]).unwrap();
            self.right_shipdate = F::deserialize_uncompressed_unchecked(&bytes[field_size..]).unwrap();
        }else{
            panic!("subcircuit_idx > self.params.num_rows");
        }
    }

    fn generate_constraints<P: PortalManager<F>>(
        &mut self,
        cs: ConstraintSystemRef<F>,
        subcircuit_idx: usize,
        pm: &mut P,
    ) -> Result<(), SynthesisError> {
        let starting_num_constraints = cs.num_constraints();

        if subcircuit_idx < self.params.num_rows  {
            let start = Instant::now();

            let rhs_var: FpVar<F> = FpVar::new_constant(ns!(cs, "rhs_var"), self.right_shipdate)?;
            let lhs_var: FpVar<F> = FpVar::new_witness(ns!(cs, "lhs_var"), || Ok(self.lineitem_table[subcircuit_idx][6]))?;
            let is_less_than_var: Boolean<F> = lhs_var.is_cmp(&rhs_var, Ordering::Less, true)?;
            let one_var: FpVar<F> = FpVar::<F>::constant(F::one());
            let zero_var: FpVar<F> = FpVar::<F>::constant(F::zero());
            let cmp_res: FpVar<F> = is_less_than_var.select(&one_var, &zero_var)?;
            pm.set(format!("lhs {subcircuit_idx} com_res"), &cmp_res)?;
            let elapsed = start.elapsed();
            println!("circuit {subcircuit_idx} Elapsed time: {:?}", elapsed);
        }else{
            panic!("subcircuit_idx > self.params.num_rows");
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
        for (i, lhs_row) in self.lineitem_table.iter().enumerate() {
            pm.start_subtrace(ConstraintSystem::new_ref());
            // 计算并存储比较结果
            let is_less_or_equal = lhs_row[6] <= self.right_shipdate;
            if is_less_or_equal {
                println!("!!!!circuit {i} is less than");
            }else{
                println!("!!!!circuit {i} is not less than");
            }
            let cmp_res_value = if is_less_or_equal { F::one() } else { F::zero() };
            let cmp_res = FpVar::new_witness(cs.clone(), || Ok(cmp_res_value)).unwrap();
            let _ = pm.set(format!("lhs {i} com_res"), &cmp_res);
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
