use anyhow::Result;
use bitcoin::hashes::Hash;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fake::Fake;
use plain_state::State;
use plain_types::sdk_authorization_ed25519_dalek::{Authorization, Signer};
use plain_types::*;
use rand::Rng;
use sdk_types::bitcoin;
use std::collections::HashMap;

pub fn random_output() -> Output {
    Output {
        address: sdk_types::Address::from(rand::thread_rng().gen::<[u8; 32]>()),
        content: sdk_types::Content::Value((0..1000).fake()),
    }
}

pub fn random_outpoint() -> sdk_types::OutPoint {
    let txid = bitcoin::Txid::from_inner(rand::thread_rng().gen::<[u8; 32]>());
    let vout = (0..10).fake();

    sdk_types::OutPoint::Deposit(bitcoin::OutPoint { txid, vout })
}

pub fn random_utxos(num_utxos: usize) -> HashMap<sdk_types::OutPoint, Output> {
    (0..num_utxos)
        .map(|_| (random_outpoint(), random_output()))
        .collect()
}

pub fn random_transaction(num_inputs: usize, num_outputs: usize) -> AuthorizedTransaction {
    use ed25519_dalek::Keypair;
    use rand::rngs::OsRng;
    use rand::Rng;
    let inputs = (0..num_inputs)
        .map(|_| sdk_types::OutPoint::Regular {
            txid: sdk_types::Txid::from(rand::thread_rng().gen::<[u8; 32]>()),
            vout: (0..256).fake(),
        })
        .collect::<Vec<_>>();
    let outputs = (0..num_outputs)
        .map(|_| random_output())
        .collect::<Vec<_>>();
    let transaction = Transaction { inputs, outputs };
    let mut csprng = OsRng {};
    let authorizations = (0..num_inputs)
        .map(|_| {
            let keypair = Keypair::generate(&mut csprng);
            let serialized_transaction = bincode::serialize(&transaction).unwrap();
            let signature = keypair.sign(&serialized_transaction);
            Authorization {
                public_key: keypair.public,
                signature,
            }
        })
        .collect::<Vec<_>>();
    AuthorizedTransaction {
        authorizations,
        transaction,
    }
}

pub fn random_body(
    env: &heed::Env,
    state: &State,
    num_transactions: usize,
    num_coinbase_outputs: usize,
) -> Result<Body> {
    const NUM_INPUTS: usize = 2;
    const NUM_OUTPUTS: usize = 2;

    let txn = env.read_txn()?;
    let mut inputs = vec![];
    for utxo in state.utxos.iter(&txn)?.take(num_transactions * NUM_INPUTS) {
        let (outpoint, output) = utxo?;
        inputs.push(outpoint);
    }
    let num_spent_utxos = NUM_INPUTS * num_transactions;
    let transactions = (0..num_transactions)
        .map(|_| random_transaction(NUM_INPUTS, NUM_OUTPUTS))
        .collect::<Vec<_>>();
    let coinbase = (0..num_coinbase_outputs)
        .map(|_| random_output())
        .collect::<Vec<_>>();
    Ok(Body::new(transactions, coinbase))
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let env = new_env().unwrap();
    let state = State::new(&env).unwrap();
    //    const NUM_UTXOS: usize = 90_000_000;
    //    let mut wtxn = env.write_txn().unwrap();
    //    for _ in 0..NUM_UTXOS {
    //        let outpoint = random_outpoint();
    //        let output = random_output();
    //        state.add_utxo(&mut wtxn, &outpoint, &output).unwrap();
    //    }
    //    wtxn.commit().unwrap();

    const NUM_TRANSACTIONS: usize = 600;
    const NUM_COINBASE_OUTPUTS: usize = 1;
    let body = random_body(&env, &state, NUM_TRANSACTIONS, NUM_COINBASE_OUTPUTS).unwrap();
    c.bench_function("validate_block", |b| {
        b.iter(|| {
            let txn = env.read_txn().unwrap();
            state.validate_body(&txn, black_box(&body)).unwrap();
        })
    });
    todo!();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);

fn new_env() -> Result<heed::Env> {
    let env_path = project_root::get_project_root()?.join("target/bench_state.mdb");
    // let _ = std::fs::remove_dir_all(&env_path);
    std::fs::create_dir_all(&env_path).unwrap();
    let env = heed::EnvOpenOptions::new()
        .map_size(16 * 1024 * 1024 * 1024) // 16GB
        .max_dbs(State::NUM_DBS)
        .open(env_path)?;
    Ok(env)
}
