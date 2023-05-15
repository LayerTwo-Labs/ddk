use bitcoin::hashes::Hash;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use fake::Fake;
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

pub fn random_body(num_transactions: usize, num_coinbase_outputs: usize) -> Body {
    const NUM_INPUTS: usize = 2;
    const NUM_OUTPUTS: usize = 2;
    let num_spent_utxos = NUM_INPUTS * num_transactions;
    let transactions = (0..num_transactions)
        .map(|_| random_transaction(NUM_INPUTS, NUM_OUTPUTS))
        .collect::<Vec<_>>();
    let coinbase = (0..num_coinbase_outputs)
        .map(|_| random_output())
        .collect::<Vec<_>>();
    Body::new(transactions, coinbase)
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let utxos = random_utxos(100);
    dbg!(utxos);
    todo!();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
