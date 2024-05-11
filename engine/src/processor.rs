use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use itertools::Itertools;
use log::debug;
use tokio::sync::Mutex;

use domain::account::Account;
use domain::amount::Amount;
use domain::config::{ClientId, TransactionId};
use domain::transaction::TransactionError::*;
use domain::transaction::{CsvError, StreamExt, Transaction, TransactionError};

type Accounts = HashMap<ClientId, (Account, HashMap<TransactionId, Transaction>)>;
/// Processes and manages transactions for multiple accounts.
#[derive(Default)]
pub struct TransactionProcessor {
	/// Stores accounts, each with its transaction history.
	/// Key: Client ID
	/// Value: Tuple of (Account, HashMap<TransactionId, Transaction>)
	accounts: Arc<Mutex<Accounts>>,
	/// Set of globally unique transaction IDs to prevent duplicates.
	global_tx_ids: Arc<Mutex<HashSet<TransactionId>>>,
}

#[derive(Debug)]
pub enum TransactionProcessorError {
	TransactionProcessingError(TransactionError),
	TransactionParsingError(CsvError),
}

trait TransactionProcessorErrorHandler {
	fn handle(error: TransactionProcessorError);
}

impl TransactionProcessor {
	/// Processes a stream of transactions from a CSV reader.
	///
	/// This function reads and parses transactions from the provided reader, handles each transaction,
	/// and returns a vector of all the resulting account states.
	///
	/// # Errors
	///
	/// Returns a `TransactionError` if an error occurs while parsing transactions or handling individual transactions.
	pub async fn process_transactions<F>(
		reader: impl domain::transaction::AsyncRead + Unpin + Send + 'static,
		error_handler: F,
	) -> Result<Vec<Account>, TransactionError>
	where
		F: Fn(TransactionProcessorError),
	{
		let mut tx_stream = Transaction::tx_stream(reader);
		let mut tx_processor = TransactionProcessor::default();
		while let Some(tx_result) = tx_stream.next().await {
			match tx_result.map_err(TransactionProcessorError::TransactionParsingError) {
				Ok(tx) => tx_processor
					.handle_transaction(tx)
					.await
					.map_err(TransactionProcessorError::TransactionProcessingError)
					.unwrap_or_else(|e| error_handler(e)),
				Err(e) => error_handler(e),
			};
		}
		let accounts = tx_processor.get_accounts();
		Ok(accounts.await)
	}

	/// Handles a single transaction by applying its effect to the relevant account.
	///
	/// # Arguments
	///
	/// * `tx` - The `Transaction` to process.
	///
	/// # Errors
	///
	/// Returns a `TransactionError` if an error occurs during processing, such as:
	/// - DuplicateGlobalTransactionId: If the transaction ID is already in the global set.
	/// - AccountFrozen: If the account associated with the transaction is frozen.
	/// - InsufficientFunds: If a withdrawal or chargeback would result in a negative balance.
	/// - IllegalStateChange: If the transaction attempts an invalid state transition.
	/// - InvalidTransactionId: If the transaction ID is invalid for the operation.
	/// - TransactionNotFound: If a dispute, resolve, or chargeback references a non-existent transaction.
	async fn handle_transaction(&mut self, tx: Transaction) -> Result<(), TransactionError> {
		debug!("Processing transaction: {:?}", &tx);
		let mut accounts = self.accounts.lock().await;
		let mut global_tx_ids = self.global_tx_ids.lock().await;

		let (account, account_txs) = accounts.entry(*tx.client_id()).or_insert_with(|| {
			(
				Account::new(*tx.client_id(), Amount::default(), Amount::default(), false),
				HashMap::new(),
			)
		});

		let result: Result<(), TransactionError> = match &tx {
			Transaction::Deposit { amount, id, .. } => {
				if global_tx_ids.contains(id) {
					Err(DuplicateGlobalTransactionId(tx.clone()))
				} else {
					account.deposit(amount.clone()).map_err(|e| (e, tx.clone()))?;
					let tx_id = tx.id();
					account_txs.insert(tx_id, tx);
					global_tx_ids.insert(tx_id);
					Ok(())
				}
			},

			Transaction::Withdrawal { amount, id, .. } => {
				if global_tx_ids.contains(id) {
					Err(DuplicateGlobalTransactionId(tx.clone()))
				} else {
					account.withdraw(amount.clone()).map_err(|e| (e, tx.clone()))?;
					let tx_id = tx.id();
					account_txs.insert(tx_id, tx);
					global_tx_ids.insert(tx_id);
					Ok(())
				}
			},

			Transaction::Dispute { id, .. } => {
				match account_txs.get_mut(id) {
					Some(tx) => match tx.amount() {
						Some(amount) => {
							//improve: these should be atomic
							account.hold(amount).map_err(|e| (e, tx.clone()))?;
							tx.set_disputed()?;
							Ok(())
						},
						None => Err(InvalidTransactionId(tx.clone())),
					},
					None => Err(TransactionNotFound(tx.clone())),
				}
			},
			Transaction::Resolve { id, .. } => match account_txs.get_mut(id) {
				Some(tx) => match tx.amount() {
					Some(amount) => {
						//improve: these should be atomic
						account.release(amount).map_err(|e| (e, tx.clone()))?;
						tx.set_resolved()?;
						Ok(())
					},
					None => Err(InvalidTransactionId(tx.clone())),
				},
				None => Err(TransactionNotFound(tx.clone())),
			},

			Transaction::Chargeback { id, .. } => match account_txs.get_mut(id) {
				Some(tx) => match tx.amount() {
					Some(amount) => {
						//improve: these should be atomic
						account.chargeback(amount).map_err(|e| (e, tx.clone()))?;
						tx.set_chargeback()?;
						account_txs.remove(id);
						Ok(())
					},
					None => Err(InvalidTransactionId(tx.clone())),
				},
				None => Err(TransactionNotFound(tx.clone())),
			},
		};

		result
	}

	/// Retrieves all accounts resolved from the input transactions.
	async fn get_accounts(&self) -> Vec<Account> {
		let accounts = self.accounts.lock().await;
		accounts.values().map(|a| a.0.clone()).collect_vec()
	}
}
#[cfg(test)]
mod tests {
	use log::error;
	use tempfile::NamedTempFile;

	use domain::amount::Amount;
	use domain::transaction::File;

	use crate::processor::{TransactionProcessor, TransactionProcessorError};

	struct TestTransactionsCsvBuilder<'a> {
		temp_file: NamedTempFile,
		transactions: Vec<Vec<&'a str>>,
	}

	const TYPE: &str = "type";
	const CLIENT: &str = "client";
	const TX: &str = "tx";
	const AMOUNT: &str = "amount";
	const DEPOSIT: &str = "deposit";
	const WITHDRAWAL: &str = "withdrawal";
	const DISPUTE: &str = "dispute";
	const RESOLVE: &str = "resolve";
	const CHARGEBACK: &str = "chargeback";
	const EMPTY: &str = "";

	impl<'a> TestTransactionsCsvBuilder<'a> {
		fn new() -> Self {
			Self {
				temp_file: NamedTempFile::new().unwrap(),
				transactions: vec![vec![TYPE, CLIENT, TX, AMOUNT]],
			}
		}
		fn deposit(mut self, client_id: &'a str, tx_id: &'a str, amount: &'a str) -> Self {
			self.transactions.push(vec![DEPOSIT, client_id, tx_id, amount]);
			self
		}
		fn withdrawal(mut self, client_id: &'a str, tx_id: &'a str, amount: &'a str) -> Self {
			self.transactions.push(vec![WITHDRAWAL, client_id, tx_id, amount]);
			self
		}
		fn dispute(mut self, client_id: &'a str, tx_id: &'a str) -> Self {
			self.transactions.push(vec![DISPUTE, client_id, tx_id, EMPTY]);
			self
		}
		fn resolve(mut self, client_id: &'a str, tx_id: &'a str) -> Self {
			self.transactions.push(vec![RESOLVE, client_id, tx_id, EMPTY]);
			self
		}
		fn chargeback(mut self, client_id: &'a str, tx_id: &'a str) -> Self {
			self.transactions.push(vec![CHARGEBACK, client_id, tx_id, EMPTY]);
			self
		}

		async fn write(self) -> Self {
			tokio::fs::write(
				self.temp_file.path(),
				self.transactions
					.iter()
					.map(|row| row.join(","))
					.collect::<Vec<String>>()
					.join("\n"),
			)
			.await
			.unwrap();
			self
		}

		async fn reader(self) -> File {
			File::open(self.temp_file.path()).await.unwrap()
		}
	}

	fn amount(value: &str) -> Amount {
		Amount::try_from(value).unwrap()
	}

	fn error_handler(e: TransactionProcessorError) {
		error!("{e:?}");
	}

	#[tokio::test]
	async fn test_process_transactions_simple() {
		enable_debug_logs();

		let transactions_csv = TestTransactionsCsvBuilder::new()
			.deposit("1", "1", "1")
			.deposit("1", "2", "1")
			.deposit("1", "3", "1")
			.withdrawal("1", "4", "1")
			.write()
			.await;

		let reader = transactions_csv.reader().await;
		let accounts =
			TransactionProcessor::process_transactions(reader, error_handler).await.unwrap();

		assert_eq!(accounts.len(), 1);

		let account = &accounts[0];
		assert_eq!(account.client_id, 1);
		assert_eq!(account.available, amount("2"));
		assert_eq!(account.held, amount("0"));
		assert_eq!(account.total(), amount("2"));
		assert!(!account.locked);
	}
	#[tokio::test]
	async fn test_process_transactions_with_disputes() {
		enable_debug_logs();

		let transactions_csv = TestTransactionsCsvBuilder::new()
			.deposit("1", "1", "1")
			.deposit("1", "2", "1")
			.deposit("1", "3", "1")
			.withdrawal("1", "4", "1")
			.dispute("1", "3")
			.write()
			.await;

		let reader = transactions_csv.reader().await;
		let accounts =
			TransactionProcessor::process_transactions(reader, error_handler).await.unwrap();

		assert_eq!(accounts.len(), 1);

		let account = &accounts[0];
		assert_eq!(account.client_id, 1);
		assert_eq!(account.available, amount("1"));
		assert_eq!(account.held, amount("1"));
		assert_eq!(account.total(), amount("2"));
		assert!(!account.locked);
	}

	#[tokio::test]
	async fn test_process_transactions_with_dispute_and_resolve() {
		enable_debug_logs();

		let transactions_csv = TestTransactionsCsvBuilder::new()
			.deposit("1", "1", "1")
			.deposit("1", "2", "1")
			.deposit("1", "3", "1")
			.withdrawal("1", "4", "1")
			.dispute("1", "3")
			.resolve("1", "3")
			.write()
			.await;

		let reader = transactions_csv.reader().await;
		let accounts =
			TransactionProcessor::process_transactions(reader, error_handler).await.unwrap();

		assert_eq!(accounts.len(), 1);

		let account = &accounts[0];
		assert_eq!(account.client_id, 1);
		assert_eq!(account.available, amount("2"));
		assert_eq!(account.held, amount("0"));
		assert_eq!(account.total(), amount("2"));
		assert!(!account.locked);
	}

	#[tokio::test]
	async fn test_process_transactions_with_dispute_and_chargeback() {
		enable_debug_logs();

		let transactions_csv = TestTransactionsCsvBuilder::new()
			.deposit("1", "1", "1")
			.deposit("1", "2", "1")
			.deposit("1", "3", "1")
			.withdrawal("1", "4", "1")
			.dispute("1", "3")
			.resolve("1", "3")
			.dispute("1", "3")
			.chargeback("1", "3")
			.write()
			.await;

		let reader = transactions_csv.reader().await;
		let accounts =
			TransactionProcessor::process_transactions(reader, error_handler).await.unwrap();

		assert_eq!(accounts.len(), 1);

		let account = &accounts[0];
		assert_eq!(account.client_id, 1);
		assert_eq!(account.available, amount("1"));
		assert_eq!(account.held, amount("0"));
		assert_eq!(account.total(), amount("1"));
		assert!(account.locked);
	}

	#[tokio::test]
	async fn test_process_transactions_with_dispute_and_resolve_and_further_dispute_and_chargeback()
	{
		enable_debug_logs();

		let transactions_csv = TestTransactionsCsvBuilder::new()
			.deposit("1", "1", "1")
			.deposit("1", "2", "1")
			.deposit("1", "3", "1")
			.withdrawal("1", "4", "1")
			.dispute("1", "3")
			.chargeback("1", "3")
			.write()
			.await;

		let reader = transactions_csv.reader().await;
		let accounts =
			TransactionProcessor::process_transactions(reader, error_handler).await.unwrap();

		assert_eq!(accounts.len(), 1);

		let account = &accounts[0];
		assert_eq!(account.client_id, 1);
		assert_eq!(account.available, amount("1"));
		assert_eq!(account.held, amount("0"));
		assert_eq!(account.total(), amount("1"));
		assert!(account.locked);
	}

	fn enable_debug_logs() {
		std::env::set_var("RUST_LOG", "debug");
		let _ = env_logger::builder().is_test(true).try_init();
	}
}
