 
## Transaction Processing Engine - Ido Flax
This Rust-based engine efficiently processes financial transactions from CSV files, handles disputes, chargebacks, and outputs the final account states. It prioritizes correctness, safety, and maintainability while being mindful of efficiency considerations.

### Usage

To build and run:
```shell
cargo build && cargo run -- transactions.csv
```

You can see the test coverage here:
```shell
cargo llvm-cov test --workspace
```
---
### Description:
The project is made up of three crates: 
- At the root we have `transaction-csv-processor` which provides the main function and CLI
- `domain` for data definitions
- `engine` for core functionality

The implementation is designed to support multiple concurrent csv streams using `csv-async` and `tokio`.
