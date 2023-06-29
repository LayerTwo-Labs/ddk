# Crates in the workspace
- `node` -- a node that integrates all of the components together.
- `net` -- networking code.
- `api` -- definition of gRPC api.
- `state` -- validation rules and state transition rules.
- `archive` -- storage for historical block data.
- `mempool` -- storage for not yet included transactions.
- `types` -- types specific to this sidechain.
- `wallet` -- library for implementing a software HDKD wallet.
- `miner` -- library for blind merge mining.
- `drivechain` -- implementation of BIP300 blind merge mining verification and
  BIP301 deposit and withdrawal verification.
- `authorization` -- implementation of a transaction
  authorization mechanism using ed25519 curve.

# Todo
- [ ] Handle reorgs
- [ ] Shorten address to 160 bits from  256 bits
