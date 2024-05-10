extern crate core;

use std::io::Write;

use clap::Parser;
use csv::WriterBuilder;

use domain::account::Account;
use domain::transaction::File;
use engine::processor::TransactionProcessor;

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

	let output_accounts = TransactionProcessor::process_transactions(reader).await.unwrap();

	let stdout = std::io::stdout();
	write_accounts(output_accounts, stdout).unwrap();
}

fn write_accounts(accounts: Vec<Account>, writer: impl Write) -> Result<(), std::io::Error> {
	let mut csv_writer = WriterBuilder::new().has_headers(true).from_writer(writer);
	for account in accounts {
		match csv_writer.serialize(account) {
			Ok(_) => {},
			Err(err) => {
				eprintln!("Error serializing account: {}", err);
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
		let result = String::from_utf8(out).unwrap(); // Convert Vec<u8> to String
		assert_eq!(expected, result);
	}
}
