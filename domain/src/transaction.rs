use core::fmt;
use std::fmt::Display;

pub use async_std::fs::File;
use csv_async::{AsyncReaderBuilder, DeserializeRecordsIntoStream, Trim};
pub use csv_async::{Error as CsvError, Result as CsvResult};
pub use futures::stream::Map;
pub use futures::stream::StreamExt;
pub use futures::Stream;
pub use futures_io::AsyncRead;
use log::error;
use rust_decimal::Decimal;
use rusty_money::Money;
use serde::de::Visitor;
use serde::ser::Error;
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use TransactionError::{AccountFrozen, InsufficientFunds};

use crate::account::AccountError;
use crate::amount::Amount;
use crate::config::{ClientId, TransactionId, CURRENCY, MAX_DECIMAL_PLACES, ROUNDING};
use crate::transaction::TransactionError::{
	IllegalStateChange, InternalError, InvalidTransactionId,
};

/// Represents the different types of transaction rows.
#[derive(Debug, Deserialize, PartialEq, Display)]
pub(crate) enum TransactionRowType {
	#[serde(rename = "deposit")]
	Deposit,
	#[serde(rename = "withdrawal")]
	Withdrawal,
	#[serde(rename = "dispute")]
	Dispute,
	#[serde(rename = "resolve")]
	Resolve,
	#[serde(rename = "chargeback")]
	Chargeback,
}
impl TransactionRowType {
	/// Checks if the transaction type should have an associated amount.
	pub(crate) fn has_amount(&self) -> bool {
		!matches!(
			self,
			TransactionRowType::Dispute
				| TransactionRowType::Resolve
				| TransactionRowType::Chargeback
		)
	}
}

/// Represents a row in the transaction CSV file.
#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct TransactionRow {
	#[serde(rename = "tx")]
	pub(crate) tx_id: TransactionId,
	#[serde(rename = "type")]
	pub(crate) tx_type: TransactionRowType,
	pub(crate) client: ClientId,
	pub(crate) amount: Option<Amount>,
}

/// Logic for deserializing an Amount from a string.
impl<'de> Deserialize<'de> for Amount {
	fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		struct AmountVisitor;

		impl<'de> Visitor<'de> for AmountVisitor {
			type Value = Amount;

			fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
				formatter.write_str("a decimal number representing the amount")
			}

			fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
			where
				E: de::Error,
			{
				let decimal = Decimal::from_str_exact(v).map_err(de::Error::custom)?;
				if decimal.scale() > MAX_DECIMAL_PLACES as u32 {
					return Err(de::Error::custom(format!(
						"Too many decimal places: {}, max allowed: {MAX_DECIMAL_PLACES}",
						v
					)));
				};

				let tx_amount = Amount::try_from(Money::from_decimal(decimal, CURRENCY))
					.map_err(|e| de::Error::custom(format!("Invalid amount: {e}")))?;
				Ok(tx_amount)
			}
		}

		deserializer.deserialize_str(AmountVisitor)
	}
}

impl Serialize for Amount {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: Serializer,
	{
		let rounded = self
			.value()
			.amount()
			.round_dp_with_strategy(MAX_DECIMAL_PLACES as u32, ROUNDING);
		serializer.serialize_str(rounded.to_string().replace(CURRENCY.symbol, "").as_str())
	}
}

/// Represents errors that can occur during transaction processing.
#[derive(Debug, PartialEq)]
pub enum TransactionError {
	/// The transaction could not be found.
	TransactionNotFound(Transaction),
	/// The transaction has already been processed.
	DuplicateGlobalTransactionId(Transaction),
	/// The transaction id refers to a wrong type of transaction.
	InvalidTransactionId(Transaction),
	/// The account does not have enough funds to complete the transaction.
	InsufficientFunds(Transaction),
	/// The transaction could not be processed due to an invalid state change.
	IllegalStateChange(Transaction),
	/// The referenced account has been frozen.
	AccountFrozen(Transaction),
	/// The transaction could not be processed due to an internal error.
	InternalError(Transaction, String),
}

/// Represents the possible states of a transaction.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TransactionState {
	/// The transaction has been successfully processed.
	Okay,
	/// The transaction has been disputed.
	Disputed,
	/// The transaction has been charged back.
	ChargedBack,
}

/// Represents a financial transaction with an associated state.
#[derive(Debug, PartialEq, Clone)]
pub enum Transaction {
	Deposit { id: TransactionId, amount: Amount, client_id: ClientId, state: TransactionState },
	Withdrawal { id: TransactionId, amount: Amount, client_id: ClientId, state: TransactionState },
	Dispute { id: TransactionId, client: ClientId },
	Resolve { id: TransactionId, client: ClientId },
	Chargeback { id: TransactionId, client: ClientId },
}

impl TryFrom<CsvResult<TransactionRow>> for Transaction {
	type Error = CsvError;

	/// Tries to convert a `TransactionRow` parsing result into a transaction.
	fn try_from(row: CsvResult<TransactionRow>) -> Result<Self, CsvError> {
		row.map(|transaction_row| {
			if !transaction_row.tx_type.has_amount() && transaction_row.amount.is_some() {
				Err(CsvError::custom(format!(
					"Transaction with type {} cannot have an amount",
					transaction_row.tx_type
				)))
			} else if transaction_row.tx_type.has_amount() && transaction_row.amount.is_none() {
				Err(CsvError::custom(format!(
					"Transaction with type {} must have an amount",
					transaction_row.tx_type
				)))
			} else {
				Ok(match transaction_row.tx_type {
					TransactionRowType::Deposit => Transaction::deposit(
						transaction_row.tx_id,
						transaction_row.amount.unwrap(),
						transaction_row.client,
					),
					TransactionRowType::Withdrawal => Transaction::withdrawal(
						transaction_row.tx_id,
						transaction_row.amount.unwrap(),
						transaction_row.client,
					),
					TransactionRowType::Dispute => {
						Transaction::dispute(transaction_row.tx_id, transaction_row.client)
					},
					TransactionRowType::Resolve => {
						Transaction::resolve(transaction_row.tx_id, transaction_row.client)
					},
					TransactionRowType::Chargeback => {
						Transaction::chargeback(transaction_row.tx_id, transaction_row.client)
					},
				})
			}
		})
		.map_err(CsvError::from)?
	}
}

impl From<(AccountError, Transaction)> for TransactionError {
	fn from((err, tx): (AccountError, Transaction)) -> Self {
		match err {
			AccountError::InsufficientFunds => InsufficientFunds(tx),
			AccountError::AccountLocked => AccountFrozen(tx),
			AccountError::Arithmetic(e) => InternalError(tx, e.to_string()),
		}
	}
}
impl Transaction {
	/// Creates a new `Deposit` transaction.
	///
	/// # Arguments
	///
	/// * `id`: The unique identifier for the transaction.
	/// * `amount`: The amount of the deposit.
	/// * `client`: The client's ID.
	pub fn deposit(id: TransactionId, amount: Amount, client: ClientId) -> Self {
		Transaction::Deposit { id, amount, client_id: client, state: TransactionState::Okay }
	}

	/// Creates a new `Withdrawal` transaction.
	///
	/// # Arguments
	///
	/// * `id`: The unique identifier for the transaction.
	/// * `amount`: The amount of the withdrawal.
	/// * `client`: The client's ID.
	pub fn withdrawal(id: TransactionId, amount: Amount, client: ClientId) -> Self {
		Transaction::Withdrawal { id, amount, client_id: client, state: TransactionState::Okay }
	}

	/// Creates a new `Dispute` transaction.
	///
	/// # Arguments
	///
	/// * `id`: The unique identifier of the transaction being disputed.
	/// * `client`: The client's ID initiating the dispute.
	pub(crate) fn dispute(id: TransactionId, client: ClientId) -> Self {
		Transaction::Dispute { id, client }
	}

	/// Creates a new `Resolve` transaction.
	///
	/// # Arguments
	///
	/// * `id`: The unique identifier of the transaction being resolved.
	/// * `client`: The client's ID for whom the dispute is being resolved.
	pub(crate) fn resolve(id: TransactionId, client: ClientId) -> Self {
		Transaction::Resolve { id, client }
	}

	/// Creates a new `Chargeback` transaction.
	///
	/// # Arguments
	///
	/// * `id`: The unique identifier of the transaction being charged back.
	/// * `client`: The client's ID initiating the chargeback.
	pub(crate) fn chargeback(id: TransactionId, client: ClientId) -> Self {
		Transaction::Chargeback { id, client }
	}

	/// Returns the transaction ID.
	pub fn id(&self) -> TransactionId {
		match self {
			Transaction::Deposit { id, .. } => *id,
			Transaction::Withdrawal { id, .. } => *id,
			Transaction::Dispute { id, .. } => *id,
			Transaction::Resolve { id, .. } => *id,
			Transaction::Chargeback { id, .. } => *id,
		}
	}

	/// Returns the transaction amount if applicable (`Deposit` or `Withdrawal`).
	///
	/// For `Dispute`, `Resolve`, and `Chargeback` transactions, returns `None`.
	pub fn amount(&self) -> Option<Amount> {
		match self {
			Transaction::Deposit { amount, .. } => Some(amount.clone()),
			Transaction::Withdrawal { amount, .. } => Some(amount.clone()),
			_ => None,
		}
	}

	/// Returns the state of the transaction, if applicable.
	///
	/// Returns the state for `Deposit` and `Withdrawal` transactions; otherwise, returns `None`.
	pub fn state(&self) -> Option<&TransactionState> {
		match self {
			Transaction::Deposit { state, .. } | Transaction::Withdrawal { state, .. } => {
				Some(state)
			},
			_ => None,
		}
	}

	/// Changes the state of a transaction based on the current state and the provided `transaction_state`.
	///
	/// # Errors
	///
	/// * Returns [`TransactionError::IllegalStateChange`] if the state transition is not allowed.
	/// * Returns [`InvalidTransactionId`] if the transaction does not have a changeable state.
	fn change_state(
		&mut self,
		transaction_state: TransactionState,
	) -> Result<(), TransactionError> {
		match self {
			Transaction::Deposit { state, .. } | Transaction::Withdrawal { state, .. } => {
				match (*state, transaction_state) {
					(TransactionState::Okay, TransactionState::Disputed)
					| (TransactionState::Disputed, TransactionState::Okay)
					| (TransactionState::Disputed, TransactionState::ChargedBack) => {
						*state = transaction_state;
						Ok(())
					},
					_ => {
						error!("Illegal state transition: {:?} -> {:?}", state, transaction_state);
						Err(IllegalStateChange(self.clone()))
					},
				}
			},
			_ => Err(InvalidTransactionId(self.clone())),
		}
	}

	/// Sets the transaction state to `Disputed`.
	pub fn set_disputed(&mut self) -> Result<(), TransactionError> {
		self.change_state(TransactionState::Disputed)
	}

	/// Sets the transaction state to `Okay`.
	pub fn set_resolved(&mut self) -> Result<(), TransactionError> {
		self.change_state(TransactionState::Okay)
	}

	/// Sets the transaction state to `ChargedBack`.
	pub fn set_chargeback(&mut self) -> Result<(), TransactionError> {
		self.change_state(TransactionState::ChargedBack)
	}

	/// Returns the client ID.
	pub fn client_id(&self) -> &ClientId {
		match self {
			Transaction::Deposit { client_id: client, .. } => client,
			Transaction::Withdrawal { client_id: client, .. } => client,
			Transaction::Dispute { client, .. } => client,
			Transaction::Resolve { client, .. } => client,
			Transaction::Chargeback { client, .. } => client,
		}
	}

	/// Stream transactions from the given reader, including errors
	pub fn tx_stream(
		reader: impl AsyncRead + Unpin + Send + 'static,
	) -> impl Stream<Item = Result<Transaction, CsvError>> {
		let csv_reader = AsyncReaderBuilder::new()
			.trim(Trim::All)
			.has_headers(true)
			.create_deserializer(reader);
		let iter: DeserializeRecordsIntoStream<_, TransactionRow> =
			csv_reader.into_deserialize::<TransactionRow>();
		iter.map(Transaction::try_from)
	}
}

#[cfg(test)]
mod tests {
	use csv_async::AsyncReaderBuilder;
	use futures::io::BufReader;
	use tokio_stream::StreamExt;

	use crate::transaction::Transaction;

	use super::*;

	#[tokio::test]
	async fn test_transaction_row_type_has_amount() {
		assert!(TransactionRowType::Deposit.has_amount());
		assert!(TransactionRowType::Withdrawal.has_amount());
		assert!(!TransactionRowType::Dispute.has_amount());
		assert!(!TransactionRowType::Resolve.has_amount());
		assert!(!TransactionRowType::Chargeback.has_amount());
	}

	#[tokio::test]
	async fn test_try_from_row() {
		let input = "type, client,tx, amount\ndeposit,1, 1, 1.1234";
		let reader = BufReader::new(input.as_bytes());
		let csv_reader = AsyncReaderBuilder::new()
			.trim(Trim::All)
			.has_headers(true)
			.create_deserializer(reader);
		let stream: DeserializeRecordsIntoStream<_, TransactionRow> = csv_reader.into_deserialize();

		let vec: Vec<Result<Transaction, CsvError>> =
			stream.map(Transaction::try_from).collect().await;

		assert!(vec.first().unwrap().is_ok());
	}

	#[tokio::test]
	async fn test_try_from_row_reject_decimal_places() {
		let input = "type, client,tx, amount\ndeposit,1, 1, 1.12345";
		let reader = BufReader::new(input.as_bytes());
		let csv_reader = AsyncReaderBuilder::new()
			.trim(Trim::All)
			.has_headers(true)
			.create_deserializer(reader);
		let stream: DeserializeRecordsIntoStream<_, TransactionRow> = csv_reader.into_deserialize();

		let vec: Vec<Result<Transaction, CsvError>> =
			stream.map(Transaction::try_from).collect().await;

		assert!(vec.first().unwrap().is_err())
	}

	#[tokio::test]
	async fn test_try_from_row_reject_negative_amount() {
		let input = "type, client,tx, amount\ndeposit,1, 1, -1.0";
		let reader = BufReader::new(input.as_bytes());
		let csv_reader = AsyncReaderBuilder::new()
			.trim(Trim::All)
			.has_headers(true)
			.create_deserializer(reader);
		let stream: DeserializeRecordsIntoStream<_, TransactionRow> = csv_reader.into_deserialize();

		let vec: Vec<Result<Transaction, CsvError>> =
			stream.map(Transaction::try_from).collect().await;

		assert!(vec.first().unwrap().is_err())
	}

	#[tokio::test]
	async fn test_change_state_deposit_open_to_disputed() {
		let mut transaction = Transaction::Deposit {
			id: 1,
			amount: Amount::try_from("50").unwrap(),
			client_id: 1,
			state: TransactionState::Okay,
		};

		let result = transaction.change_state(TransactionState::Disputed);

		assert!(result.is_ok());
		assert_eq!(transaction.state().unwrap(), &TransactionState::Disputed);
	}

	#[tokio::test]
	async fn test_change_state_withdrawal_disputed_to_okay() {
		let mut transaction = Transaction::Withdrawal {
			id: 1,
			amount: Amount::try_from("50").unwrap(),
			client_id: 1,
			state: TransactionState::Disputed,
		};

		let result = transaction.change_state(TransactionState::Okay);

		assert!(result.is_ok());
		assert_eq!(transaction.state().unwrap(), &TransactionState::Okay);
	}

	#[tokio::test]
	async fn test_change_state_invalid_state_transition() {
		let mut transaction = Transaction::Deposit {
			id: 1,
			amount: Amount::try_from("50").unwrap(),
			client_id: 1,
			state: TransactionState::ChargedBack,
		};

		let result = transaction.change_state(TransactionState::Okay);

		assert_eq!(result, Err(IllegalStateChange(transaction.clone())));
		// State shouldn't have changed
		assert_eq!(transaction.state().unwrap(), &TransactionState::ChargedBack);
	}
}
