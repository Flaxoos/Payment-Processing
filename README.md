 
## Transaction Processing Engine Submission - Ido Flax
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

### Caveats
- It is necessary to keep a transaction cache in memory as long as new transaction are coming in, as they can refer to any previous transactions, but because there is no persistence layer, the transaction cache can grow to the point of exceeding memory limits.
- In `processor.rs:131/144/157`, i mention that the operations of modifying the account balance and the transaction state should be atomic. This could be implemented using DB transactions for example.
- I wasn't sure what "Likewise, transaction IDs (tx) are globally unique" means but i allow for both interpretations, see the comment when handling this case in `processor.rs:49` 

Thank you for reviewing my submission!

Ido Flax