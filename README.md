# pod-network-rs
Implementation of the Pod network in Rust.

## How Pod Network Works
Pod Network operates with two core components:

1. `pod-client`:
Broadcasts <WRITE tx> messages to replicas to initiate transactions.
Gathers votes from replicas and confirms transactions once a quorum (α) is achieved.
Tracks the pod state (T), including confirmed transactions and the past-perfect round (r_perf).

2. `pod-replica`:
Listens for client requests (e.g., <WRITE tx>, <CONNECT>).
Responds with signed votes, including timestamps (ts) and sequence numbers (sn) for ordering.


## Key Terms
- `Votes`: Replicas issue signed votes with timestamps and sequence numbers.   
- `Quorum`: Transactions are confirmed with α votes, ensuring system-wide consistency.
- `Ordering`: Timestamps order votes, with the median timestamp setting the confirmation round (r_conf).
- `Past-Perfect Round (r_perf)`: Tracks the latest round with all confirmed transactions known to the client.
- `Sequence Number (SN)`:
- `Confirmation Round (r_conf)`:

## Codebase Structure
The project is organized as follows:

```
pod-network-rs/
    ├── crates  
    │   ├── pod-client/             # pod-client logic
    │   │   ├── src/                
    │   │   │   ├── main.rs         # pod-client binary
    │   │   │   ├── client.rs       # Client state management 
    │   │   │   ├── network.rs      # Client network handling
    │   │   │   └── rest_api.rs     # Client REST API
    │   │   └── Cargo.toml/         # pod-client dependencies
    │   │   
    │   ├── pod-replica/            # pod-replica logic
    │   │   ├── src/                
    │   │   │   ├── lib.rs          # Replica state management
    │   │   │   ├── network.rs      # Replica network handling
    │   │   │   └── main.rs         # pod-replica binary
    │   │   └── Cargo.toml/         # pod-replica dependencies
    │   │   
    │   ├── pod-common/             # Shared utilities and types
    │   │   ├── src/                
    │   │   │   ├── lib.rs
    │   │   │   ├── crypto.rs       # Cryptographic functions
    │   │   │   ├── errors.rs       # Custom error types
    │   │   │   ├── network.rs      # Network traits 
    │   │   │   └── types.rs        # Shared data structures (e.g., Message, Transaction)       
    │   │   └── Cargo.toml/         # pod-common dependencies
    ├── src/ 
    │   └── main.rs          # Entry point for binaries
    │             
    ├── Cargo.toml           # Dependencies and metadata
    └── README.md            # This file
```

## Running the Binaries

### Prerequisites

`Rust`: Install rustc and cargo (see rust-lang.org).
`Dependencies`: Run cargo build to compile dependencies.

### Running pod-replica
1. Build the project:
```bash 
cargo build --release
```

2. Launch a replica:
```bash
cargo run --bin pod-replica -- --address 127.0.0.1:8080 --seed <32_byte_number>
```
- `address`: Specifies the IP and port for the replica.
- `seed`: seed generates the public / private keys for the replica.

### Running pod-client
1. Build the project:
```bash 
cargo build --release
```

2. Launch a client:
```bash
cargo run --bin pod-client -- --config <path_to_yaml>  --alpha 3 --beta 1
```
- config: Path to the replicas config yaml file, i.e., (public_key, url) pairs.
- alpha: Quorum size for confirming transactions.
- beta: Byzantine fault tolerance threshold.

## Features Implemented from the Research Paper
This codebase includes the following features from the Pod Network paper:

- `Transaction Writing and Confirmation`:
Clients send <WRITE tx> messages, replicas respond with signed votes, and transactions are confirmed with α votes using the median timestamp for r_conf.
- `Vote Verification`:
Votes are signed and verified with public-key cryptography, ensuring sequence numbers and timestamps are valid.
- `Backlog Handling`:
Out-of-order votes are stored and processed when the sequence aligns.
- `Pod State Management`:
Clients maintain T with confirmed transactions and r_perf, updated incrementally.
- `Fault Tolerance`:
Tolerates up to β Byzantine replicas, with α >= 4β + 1 for consistency and liveness.
- `Efficient Reads`:
Cached pod state enables fast read() operations.
- `Networking`:
Uses WebSockets for real-time communication and heartbeats to detect disconnections.

