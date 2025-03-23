use ark_ip_proofs::tipa::TIPA;
use distributed_prover::{
    aggregation::AggProvingKey,
    coordinator::{CoordinatorStage0State, FinalAggState, G16ProvingKeyGenerator},
    poseidon_util::{
        gen_merkle_params, PoseidonTreeConfig as TreeConfig, PoseidonTreeConfigVar as TreeConfigVar,
    },
    test_circuit::{ZkDbSqlCircuit, ZkDbSqlCircuitParams},
    util::{cli_filenames::*, deserialize_from_path, serialize_to_path, serialize_to_paths},
    worker::{Stage0Response, Stage1Response},
    CircuitWithPortals,
};
use sha2::Sha256;
use std::time::Instant;
use std::{io, path::PathBuf};

use ark_bls12_381::{Bls12_381 as E, Fr};
use ark_std::{end_timer, start_timer};
use clap::{Parser, Subcommand};
use rayon::prelude::*;

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generates the Groth16 proving keys and aggregation key  for a test circuit consisting of
    /// `n` subcircuits. Places them in coord-state-dir
    /// cargo run  --bin test_co gen-keys --g16-pk-dir ./pk-test  --coord-state-dir ./co-test --num-rows 2
    GenKeys {
        /// Directory where the Groth16 proving keys will be stored
        #[clap(long, value_name = "DIR")]
        g16_pk_dir: PathBuf,

        /// Directory where the coordinator's intermediate state is stored.
        #[clap(long, value_name = "DIR")]
        coord_state_dir: PathBuf,


        #[clap(long, value_name = "NUM")]
        num_rows: usize,
    },

    /// Begins stage0 for a random proof for a large circuit with the given parameters. This
    /// produces _worker request packages_ which are processed in parallel by worker nodes.
    /// cargo run  --bin test_co start-stage0 --req-dir ./req  --coord-state-dir ./co-test
    StartStage0 {
        /// Directory where the coordinator's intermediate state is stored.
        #[clap(short, long, value_name = "DIR")]
        coord_state_dir: PathBuf,

        /// Directory where the worker requests are stored
        #[clap(short, long, value_name = "DIR")]
        req_dir: PathBuf,
    },

    /// Process the stage0 responses from workers and produce stage1 reqeusts
    /// cargo run  --bin test_co start-stage1 --req-dir ./req  --coord-state-dir ./co-test --resp-dir ./resp-s0
    StartStage1 {
        /// Directory where the coordinator's intermediate state is stored.
        #[clap(long, value_name = "DIR")]
        coord_state_dir: PathBuf,

        /// Directory where the worker requests are stored
        #[clap(long, value_name = "DIR")]
        req_dir: PathBuf,

        /// Directory where the worker responses are stored
        #[clap(long, value_name = "DIR")]
        resp_dir: PathBuf,
    },

    /// Process the stage1 responses from workers and produce a final aggregate
    /// cargo run  --bin test_co end-proof --coord-state-dir ./co-test --resp-dir ./resp-s1
    EndProof {
        /// Directory where the coordinator's intermediate state is stored.
        #[clap(short, long, value_name = "DIR")]
        coord_state_dir: PathBuf,

        #[clap(short, long, value_name = "DIR")]
        resp_dir: PathBuf,
    },
}

// Checks the test circuit parameters and puts them in a struct
fn gen_test_circuit_params(
    num_rows: usize,
) -> ZkDbSqlCircuitParams {
    assert!(num_rows > 1, "num. of subcircuits MUST be > 1");

    ZkDbSqlCircuitParams {
        num_rows: num_rows,
        sort_column_idx: 0,
    }
}

/// Generates all the Groth16 proving and committing keys keys that the workers will use
fn generate_g16_pks(
    circ_params: ZkDbSqlCircuitParams,
    g16_pk_dir: &PathBuf,
    coord_state_dir: &PathBuf,
) {
    
    let mut rng = rand::thread_rng();
    let tree_params = gen_merkle_params();

    // Make an empty circuit of the correct size
    let circ = <ZkDbSqlCircuit<Fr> as CircuitWithPortals<Fr>>::new(&circ_params);
    let num_subcircuits = <ZkDbSqlCircuit<Fr> as CircuitWithPortals<Fr>>::num_subcircuits(&circ);
    println!("Making a test circuit with {num_subcircuits} subcircuits");
    let generator = G16ProvingKeyGenerator::<TreeConfig, TreeConfigVar, E, _>::new(
        circ.clone(),
        tree_params.clone(),
    );


    let first_start = Instant::now(); 
    let first_pk = generator.gen_pk(&mut rng, 0);
    let elapsed = first_start.elapsed();
    println!("first_pk Elapsed time: {:?}", elapsed);

    let middle_start = Instant::now(); 
    let middle_pk = generator.gen_pk(&mut rng, 1);
    let elapsed = middle_start.elapsed();
    println!("middle_pk Elapsed time: {:?}", elapsed);

    let last_start = Instant::now(); 
    let last_pk = generator.gen_pk(&mut rng, num_subcircuits-1);
    let elapsed = last_start.elapsed();
    println!("last_pk Elapsed time: {:?}", elapsed);


    // Now save them

    let sort_range = 1..num_subcircuits-1;
    // println!("Writing {num_subcircuits} sort proving keys");
    // serialize_to_paths(&first_pk, g16_pk_dir, G16_PK_FILENAME_PREFIX, sort_range.clone()).unwrap();
    // serialize_to_paths(&first_pk.ck,g16_pk_dir,G16_CK_FILENAME_PREFIX,sort_range.clone(),).unwrap();
    serialize_to_path(&first_pk, g16_pk_dir, G16_PK_FILENAME_PREFIX, Some(0)).unwrap();
    serialize_to_path(&first_pk.ck,g16_pk_dir,G16_CK_FILENAME_PREFIX,Some(0),).unwrap();
    serialize_to_paths(&middle_pk, g16_pk_dir, G16_PK_FILENAME_PREFIX, sort_range.clone()).unwrap();
    serialize_to_paths(&middle_pk.ck,g16_pk_dir,G16_CK_FILENAME_PREFIX,sort_range.clone()).unwrap();
    serialize_to_path(&last_pk, g16_pk_dir, G16_PK_FILENAME_PREFIX, Some(num_subcircuits-1)).unwrap();
    serialize_to_path(&last_pk.ck,g16_pk_dir,G16_CK_FILENAME_PREFIX,Some(num_subcircuits-1),).unwrap();




    // To generate the aggregation key, we need an efficient G16 pk fetcher. Normally this hits
    // disk, but this might take a long long time.
    let pk_fetcher = |subcircuit_idx: usize| {
        if subcircuit_idx ==0 {
            &first_pk
        }else if subcircuit_idx ==num_subcircuits-1{
            &last_pk
        }else{
            &middle_pk
        }
    };

    // Construct the aggregator commitment key
    let agg_start = Instant::now();
    let agg_ck = {
        let (tipp_pk, _tipp_vk) = TIPA::<E, Sha256>::setup(num_subcircuits, &mut rng).unwrap();
        AggProvingKey::new(tipp_pk, pk_fetcher)
    };
    let elapsed = agg_start.elapsed();
    println!("agg_ck construct Elapsed time: {:?}", elapsed);

    // Save the aggregator key
    println!("Writing aggregation key");
    serialize_to_path(&agg_ck, coord_state_dir, AGG_CK_FILENAME_PREFIX, None).unwrap();
}

fn begin_stage0(worker_req_dir: &PathBuf, coord_state_dir: &PathBuf) -> io::Result<()> {
    let mut rng = rand::thread_rng();
    let stage0_timer = start_timer!(|| "Begin Stage0");

    let circ_params_timer = start_timer!(|| "Deserializing circuit parameters");
    // Get the circuit parameters determined at Groth16 PK generation
    let circ_params = deserialize_from_path::<ZkDbSqlCircuitParams>(
        &coord_state_dir,
        TEST_CIRC_PARAM_FILENAME_PREFIX,
        None,
    )
    .unwrap();
    end_timer!(circ_params_timer);

    let num_subcircuits = (circ_params.num_rows+63)/64;

    let rand_circuit_timer =
        start_timer!(|| format!("Sampling a random Circuit with parapms {circ_params}"));
    // Make a random circuit with the given parameters
    println!("Making a random circuit");
    let circ = <ZkDbSqlCircuit<Fr> as CircuitWithPortals<Fr>>::rand(&mut rng, &circ_params);
    end_timer!(rand_circuit_timer);

    // Make the stage0 coordinator state
    println!("Building stage0 state");
    let stage0_state = CoordinatorStage0State::<E, _>::new::<TreeConfig>(circ);

    // Sender sends stage0 requests containing the subtraces. Workers will commit to these
    let start = start_timer!(|| format!("Generating stage0 requests with params {circ_params}"));
    let reqs = (0..num_subcircuits)
        .into_par_iter()
        .map(|subcircuit_idx| stage0_state.gen_request(subcircuit_idx))
        .collect::<Vec<_>>();
    end_timer!(start);

    let write_timer = start_timer!(|| format!("Writing stage0 requests with params {circ_params}"));
    reqs.into_par_iter()
        .enumerate()
        .for_each(|(subcircuit_idx, req)| {
            serialize_to_path(
                &req,
                worker_req_dir,
                STAGE0_REQ_FILENAME_PREFIX,
                Some(subcircuit_idx),
            )
            .unwrap()
        });
    end_timer!(write_timer);

    // Save the coordinator state
    let write_timer = start_timer!(|| format!("Writing coordinator state"));
    serialize_to_path(
        &stage0_state,
        coord_state_dir,
        STAGE0_COORD_STATE_FILENAME_PREFIX,
        None,
    )?;
    end_timer!(write_timer);
    end_timer!(stage0_timer);

    Ok(())
}

fn process_stage0_resps(coord_state_dir: &PathBuf, req_dir: &PathBuf, resp_dir: &PathBuf) {
    let tree_params = gen_merkle_params();

    // Get the circuit parameters determined at Groth16 PK generation
    let circ_params = deserialize_from_path::<ZkDbSqlCircuitParams>(
        &coord_state_dir,
        TEST_CIRC_PARAM_FILENAME_PREFIX,
        None,
    )
    .unwrap();

    let num_subcircuits = (circ_params.num_rows+63)/64;

    // Deserialize the coordinator's state and the aggregation key
    let coord_state = deserialize_from_path::<CoordinatorStage0State<E, ZkDbSqlCircuit<Fr>>>(
        coord_state_dir,
        STAGE0_COORD_STATE_FILENAME_PREFIX,
        None,
    )
    .unwrap();
    let super_com_key = {
        let agg_ck = deserialize_from_path::<AggProvingKey<E>>(
            coord_state_dir,
            AGG_CK_FILENAME_PREFIX,
            None,
        )
        .unwrap();
        agg_ck.tipp_pk
    };

    // Collect all the repsonses into a single vec. They're tiny, so this is fine.
    let stage0_resps = (0..num_subcircuits)
        .into_par_iter()
        .map(|subcircuit_idx| {
            deserialize_from_path::<Stage0Response<E>>(
                resp_dir,
                STAGE0_RESP_FILENAME_PREFIX,
                Some(subcircuit_idx),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();

    // Process the responses and get a new coordinator state
    let new_coord_state =
        coord_state.process_stage0_responses(&super_com_key, tree_params, &stage0_resps);

    // Create all the stage1 requests
    let start = start_timer!(|| format!(
        "Generating stage1 requests for circuit with params {circ_params}"
    ));
    let reqs = (0..num_subcircuits)
        .into_par_iter()
        .map(|subcircuit_idx| new_coord_state.gen_request(subcircuit_idx))
        .collect::<Vec<_>>();
    end_timer!(start);

    reqs.into_par_iter()
        .enumerate()
        .for_each(|(subcircuit_idx, req)| {
            serialize_to_path(
                &req,
                req_dir,
                STAGE1_REQ_FILENAME_PREFIX,
                Some(subcircuit_idx),
            )
            .unwrap()
        });

    // Convert the coordinator state to an aggregator state and save it
    let final_agg_state = new_coord_state.into_agg_state();
    serialize_to_path(
        &final_agg_state,
        coord_state_dir,
        FINAL_AGG_STATE_FILENAME_PREFIX,
        None,
    )
    .unwrap();
}

fn process_stage1_resps(coord_state_dir: &PathBuf, resp_dir: &PathBuf) {
    // Get the circuit parameters determined at Groth16 PK generation
    let circ_params = deserialize_from_path::<ZkDbSqlCircuitParams>(
        &coord_state_dir,
        TEST_CIRC_PARAM_FILENAME_PREFIX,
        None,
    )
    .unwrap();
    let num_subcircuits = (circ_params.num_rows+63)/64;

    // Deserialize the coordinator's final state, the aggregation key
    let final_agg_state = deserialize_from_path::<FinalAggState<E>>(
        coord_state_dir,
        FINAL_AGG_STATE_FILENAME_PREFIX,
        None,
    )
    .unwrap(); 
    let agg_ck =
        deserialize_from_path::<AggProvingKey<E>>(coord_state_dir, AGG_CK_FILENAME_PREFIX, None)
            .unwrap();

    // Collect all the stage1 repsonses into a single vec. They're tiny (Groth16 proofs), so this
    // is fine.
    let stage1_resps = (0..num_subcircuits)
        .into_par_iter()
        .map(|subcircuit_idx| {
            deserialize_from_path::<Stage1Response<E>>(
                resp_dir,
                STAGE1_RESP_FILENAME_PREFIX,
                Some(subcircuit_idx),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();

    // Compute the aggregate
    let start_instant = Instant::now();
    let start =
        start_timer!(|| format!("Aggregating proofs for circuit with params {circ_params}"));
    let agg_proof = final_agg_state.gen_agg_proof(&agg_ck, &stage1_resps);
    end_timer!(start);
    let elapsed = start_instant.elapsed();
    println!("Aggregating proofs elapsed time: {:?}", elapsed);
    // Save the proof
    serialize_to_path(&agg_proof, coord_state_dir, FINAL_PROOF_PREFIX, None).unwrap();
}

fn main() {
    // println!("Rayon num threads: {}", rayon::current_num_threads());

    let args = Args::parse();
    let start = start_timer!(|| format!("Running coordinator"));

    match args.command {
        Command::GenKeys {
            g16_pk_dir,
            coord_state_dir,
            num_rows,
        } => {
            let start = Instant::now();
            // Make the circuit params and save them to disk
            let circ_params: ZkDbSqlCircuitParams = gen_test_circuit_params(num_rows);
            serialize_to_path(
                &circ_params,
                &coord_state_dir,
                TEST_CIRC_PARAM_FILENAME_PREFIX,
                None,
            )
            .unwrap();

            // Now run the subcommand
            generate_g16_pks(circ_params, &g16_pk_dir, &coord_state_dir);
            let elapsed = start.elapsed();
            println!("!!!GenKeys Elapsed time: {:?}", elapsed);
        },

        Command::StartStage0 {
            req_dir,
            coord_state_dir,
        } => {
            let start = Instant::now();
            begin_stage0(&req_dir, &coord_state_dir).unwrap();
            let elapsed = start.elapsed();
            println!("!!!StartStage0 Elapsed time: {:?}", elapsed);
        },

        Command::StartStage1 {
            resp_dir,
            coord_state_dir,
            req_dir,
        } => {
            let start = Instant::now();
            process_stage0_resps(&coord_state_dir, &req_dir, &resp_dir);
            let elapsed = start.elapsed();
            println!("!!!StartStage1 Elapsed time: {:?}", elapsed);
        },

        Command::EndProof {
            coord_state_dir,
            resp_dir,
        } => {
            let start = Instant::now();
            process_stage1_resps(&coord_state_dir, &resp_dir);
            let elapsed = start.elapsed();
            println!("!!!End Proof Elapsed time: {:?}", elapsed);
        },
    }

    end_timer!(start);
}
