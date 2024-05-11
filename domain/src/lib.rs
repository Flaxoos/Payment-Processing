extern crate core;
#[macro_use]
extern crate enum_display_derive;

pub mod account;
pub mod amount;
pub mod config;
pub mod transaction;

#[cfg(test)]
mod tests {
	use rusty_money::Money;
	use serde::ser::Error;

	use crate::amount::{Amount, AmountResult};
	use crate::config::CURRENCY;
	use crate::transaction::Transaction;
	use crate::transaction::{CsvError, CsvResult, TransactionRow, TransactionRowType};

	fn amount() -> Option<Amount> {
		Some(Amount::try_from(Money::from_str("0.1", CURRENCY).unwrap()).unwrap())
	}

	fn amount_of(value: &str) -> AmountResult {
		Amount::try_from(Money::from_str(value, CURRENCY).unwrap())
	}

	fn row(tx_type: TransactionRowType, with_amount: bool) -> CsvResult<TransactionRow> {
		Ok(TransactionRow {
			client: 2,
			tx_id: 1,
			tx_type,
			amount: if with_amount { amount() } else { None },
		})
	}
	#[test]
	fn test_no_negative_tx_amounts() {
		assert!(amount_of("0.1").is_ok());
		assert!(amount_of("0.0").is_ok());
		assert!(amount_of("-0.1").is_err());
	}

	#[test]
	fn test_transaction_from_row_amounts_for_types() {
		assert!(Transaction::try_from(row(TransactionRowType::Deposit, true)).is_ok());
		assert!(Transaction::try_from(row(TransactionRowType::Withdrawal, true)).is_ok());
		assert!(Transaction::try_from(row(TransactionRowType::Dispute, false)).is_ok());
		assert!(Transaction::try_from(row(TransactionRowType::Resolve, false)).is_ok());
		assert!(Transaction::try_from(row(TransactionRowType::Chargeback, false)).is_ok());

		assert!(Transaction::try_from(row(TransactionRowType::Deposit, false)).is_err());
		assert!(Transaction::try_from(row(TransactionRowType::Withdrawal, false)).is_err());
		assert!(Transaction::try_from(row(TransactionRowType::Dispute, true)).is_err());
		assert!(Transaction::try_from(row(TransactionRowType::Resolve, true)).is_err());
		assert!(Transaction::try_from(row(TransactionRowType::Chargeback, true)).is_err());
	}

	#[test]
	fn test_transaction_from_row() {
		let row = TransactionRow {
			client: 2,
			tx_id: 1,
			tx_type: TransactionRowType::Deposit,
			amount: Some(Amount::try_from(Money::from_str("0.1", CURRENCY).unwrap()).unwrap()),
		};
		assert_eq!(
			Transaction::try_from(Ok(row)).unwrap(),
			Transaction::deposit(
				1,
				Amount::try_from(Money::from_str("0.1", CURRENCY).unwrap()).unwrap(),
				2
			)
		);

		let row = TransactionRow {
			client: 2,
			tx_id: 1,
			tx_type: TransactionRowType::Dispute,
			amount: Some(Amount::try_from(Money::from_str("0.1", CURRENCY).unwrap()).unwrap()),
		};
		assert!(Transaction::try_from(Ok(row)).is_err());

		assert!(Transaction::try_from(Err(CsvError::custom("whatever".to_string()))).is_err());
	}
}
