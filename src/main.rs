extern crate core;

use std::io::Write;

use clap::Parser;
use csv::WriterBuilder;

use domain::account::Account;
use domain::transaction::TransactionError::{
	AccountFrozen, DuplicateGlobalTransactionId, IllegalStateChange, InsufficientFunds,
	InvalidTransactionId, TransactionNotFound,
};
use domain::transaction::{File, TransactionError};
use engine::processor::{TransactionProcessor, TransactionProcessorError};
use log::error;
use TransactionError::InternalError;
use TransactionProcessorError::{TransactionParsingError, TransactionProcessingError};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
	extra: Vec<String>,
}

#[tokio::main]
async fn main() {
	let args = Args::parse();

	let transactions_csv = args.extra.first().expect("No transactions file provided");
	let reader = File::open(transactions_csv).await.unwrap();

	let output_accounts =
		TransactionProcessor::process_transactions(reader, error_handler).await.unwrap();

	let stdout = std::io::stdout();
	write_accounts(output_accounts, stdout).unwrap();
}

fn error_handler(e: TransactionProcessorError) {
	match e {
		TransactionProcessingError(e) => {
			match e {
				TransactionNotFound(tx) => {
					error!("Ignoring transaction referencing unknown transaction {:?}: ", &tx);
				},
				DuplicateGlobalTransactionId(tx) => {
					// or panic, depending on the meaning of "Likewise, transaction IDs (tx) are globally unique", as in, should it be guaranteed ot is it guaranteed.
					error!("Found duplicate global transaction id in: {:?}: ", &tx);
				},
				InvalidTransactionId(tx) => {
					panic!("Error: Transaction reference is wrong for transaction {:?}", &tx);
				},
				InsufficientFunds(tx) => {
					error!("Insufficient funds for transaction {:?}: ", &tx);
				},
				IllegalStateChange(tx) => {
					panic!("Error: Illegal state change for transaction {:?}: ", &tx);
				},
				AccountFrozen(tx) => {
					error!("Account frozen for transaction {:?}: ", &tx);
				},
				InternalError(tx, s) => {
					panic!("Internal Error processing transaction {:?}: {}", &tx, s);
				},
			}
		},
		TransactionParsingError(e) => {
			eprintln!("Error parsing transaction: {:?}", e);
		},
	}
}

fn write_accounts(accounts: Vec<Account>, writer: impl Write) -> Result<(), std::io::Error> {
	let mut csv_writer = WriterBuilder::new().has_headers(true).from_writer(writer);
	for account in accounts {
		match csv_writer.serialize(account) {
			Ok(()) => {},
			Err(err) => {
				eprintln!("Error serializing account: {err}");
				let _ = std::io::stderr().write_all(err.to_string().as_bytes());
			},
		}
	}
	csv_writer.flush()?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::io::BufWriter;

	use domain::account::Account;
	use domain::amount::Amount;

	use crate::write_accounts;

	#[test]
	fn test_write_accounts() {
		let available = Amount::try_from("1.10010").unwrap();
		let held = Amount::try_from("2.1001").unwrap();
		let account = Account::new(1, available, held, false);
		let accounts = vec![account];
		let mut out = Vec::new();
		let writer = BufWriter::new(&mut out);
		write_accounts(accounts, writer).unwrap();

		let expected = "client,available,held,total,locked\n1,1.1001,2.1001,3.2002,false\n";
		let result = String::from_utf8(out).unwrap();
		assert_eq!(expected, result);
	}
}
