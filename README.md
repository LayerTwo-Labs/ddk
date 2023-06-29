# Crates in the workspace
- `cli` -- executable CLI for interacting with the node.
- `node` -- a node that integrates all of the components together.
- `main` -- executable entry point.
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

# Todo
- [ ] Handle reorgs
- [ ] Shorten address to 160 bits from  256 bits
- [ ] Move sdk_types into this workspace
- [ ] Move bin crates out of this workspace
