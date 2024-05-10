use log::debug;
use AccountError::InsufficientFunds;
use crate::account::AccountError::AccountLocked;

use crate::amount::{Amount, AmountError};
use crate::config::ClientId;

/// Represents the different errors that can occur with an account.
#[derive(Debug, PartialEq)]
pub enum AccountError {
	/// The account is locked and cannot be modified.
	AccountLocked,
	/// The account has insufficient funds for the requested operation.
	InsufficientFunds,
}

impl From<AmountError> for AccountError {
	/// Converts an `AmountError` into a corresponding `AccountError`.
	///
	/// This is used to handle cases where an operation on an `Amount` results in an error
	/// that needs to be represented as an `AccountError`.
	fn from(value: AmountError) -> Self {
		match value {
			AmountError::NegativeValue(money) => unreachable!("{} Should not be negative", money),
			AmountError::SubtractToNegative(_, _) => InsufficientFunds,
		}
	}
}

/// Represents a financial account with available, held, and total balances.
#[derive(Debug, serde::Serialize, Clone)]
pub struct Account {
	#[serde(rename = "client")]
	pub client_id: ClientId,
	pub available: Amount,
	pub held: Amount,
	pub total: Amount,
	pub locked: bool,
}

impl Account {
	/// Creates a new `Account`.
	///
	/// # Arguments
	///
	/// * `client_id` - The unique identifier for the client.
	/// * `available` - The initial available balance of the account.
	/// * `held` - The initial held balance of the account.
	/// * `locked` - Whether the account is initially locked.
	pub fn new(client_id: ClientId, available: Amount, held: Amount, locked: bool) -> Self {
		let mut total_money = available.clone();
		total_money.add_assign(held.clone());
		Self { client_id, available, held, total: total_money, locked }
	}

	/// Deposits an `amount` into the account's `available` balance.
	///
	/// # Errors
	///
	/// Returns [`AccountLocked`] if the account is locked.
	pub fn deposit(&mut self, amount: Amount) -> Result<(), AccountError> {
		if self.locked {
			Err(AccountLocked)
		} else {
			debug!("Depositing {:?} to account {:?}", amount, self.client_id);
			self.available.add_assign(amount.clone());
			debug!("Current account state after deposit: {:?}", self);
			Ok(())
		}
	}

	/// Withdraws an `amount` from the account's `available` balance.
	///
	/// # Errors
	///
	/// Returns [`AccountLocked`] if the account is locked.
	/// Returns [`InsufficientFunds`] if the withdrawal would result in a negative balance.
	pub fn withdraw(&mut self, amount: Amount) -> Result<(), AccountError> {
		if self.locked {
			Err(AccountLocked)
		} else {
			debug!("Withdrawing {:?} from account {:?}", amount, self.client_id);
			self.available.checked_sub_assign(amount.clone())?;
			debug!("Current account state after withdraw: {:?}", self);
			Ok(())
		}
	}

	/// Holds an `amount` from the account's `available` balance, transferring it to the `held` balance.
	///
	/// # Errors
	///
	/// Returns [`AccountLocked`] if the account is locked.
	/// Returns [`InsufficientFunds`] if the hold would result in a negative available balance.
	pub fn hold(&mut self, amount: Amount) -> Result<(), AccountError> {
		if self.locked {
			Err(AccountLocked)
		} else {
			debug!("Holding {:?} from account {:?}", amount, self.client_id);
			self.held.add_assign(amount.clone());
			self.available.checked_sub_assign(amount)?;
			debug!("Current account state after hold: {:?}", self);
			Ok(())
		}
	}

	/// Releases a previously held `amount` back to the `available` balance.
	///
	/// # Errors
	///
	/// Returns [`AccountLocked`] if the account is locked.
	/// Returns [`InsufficientFunds`] if the release would result in a negative held balance.
	pub fn release(&mut self, amount: Amount) -> Result<(), AccountError> {
		if self.locked {
			Err(AccountLocked)
		} else {
			debug!("Releasing {:?} from account {:?}", amount, self.client_id);
			self.held.checked_sub_assign(amount.clone())?;
			self.available.add_assign(amount);
			debug!("Current account state after release: {:?}", self);
			Ok(())
		}
	}

	/// Charges back a held `amount`, deducting it from the `held` balance and freezing the account.
	///
	/// # Errors
	///
	/// Returns [`AccountLocked`] if the account is already locked.
	/// Returns [`InsufficientFunds`] if the chargeback would result in a negative held balance.
	pub fn chargeback(&mut self, amount: Amount) -> Result<(), AccountError> {
		if self.locked {
			Err(AccountLocked)
		} else {
			debug!("Charging back {:?} from account {:?}", amount, self.client_id);
			self.held.checked_sub_assign(amount.clone())?;
			self.locked = true;
			debug!("Current account state after chargeback: {:?}", self);
			Ok(())
		}
	}

	/// Calculates and returns the total balance (`available` + `held`) of the account.
	pub fn total(&self) -> Amount {
		let mut total = Amount::default();
		total.add_assign(self.available.clone());
		total.add_assign(self.held.clone());
		total
	}
}

#[cfg(test)]
mod tests {
	use crate::account::AccountError::AccountLocked;
	use super::*;

	#[test]
	fn test_new_account() {
		let client_id = 1;
		let available = Amount::try_from("100.0").unwrap();
		let held = Amount::try_from("20.0").unwrap();
		let locked = false;

		let account = Account::new(client_id, available.clone(), held.clone(), locked);

		assert_eq!(account.client_id, client_id);
		assert_eq!(account.available, available);
		assert_eq!(account.held, held);
		assert_eq!(account.total, Amount::try_from("120.0").unwrap());
		assert_eq!(account.locked, locked);
	}

	#[test]
	fn test_deposit() {
		let client_id = 1;
		let mut account = Account::new(client_id, Amount::default(), Amount::default(), false);
		let deposit_amount = Amount::try_from("50.0").unwrap();

		account.deposit(deposit_amount.clone()).unwrap();

		assert_eq!(account.available, deposit_amount);
	}

	#[test]
	fn test_withdraw() {
		let client_id = 1;
		let mut account =
			Account::new(client_id, Amount::try_from("100.0").unwrap(), Amount::default(), false);
		let withdraw_amount = Amount::try_from("30.0").unwrap();

		account.withdraw(withdraw_amount.clone()).unwrap();

		assert_eq!(account.available, Amount::try_from("70.0").unwrap());
	}

	#[test]
	fn test_hold() {
		let client_id = 1;
		let mut account =
			Account::new(client_id, Amount::try_from("100.0").unwrap(), Amount::default(), false);
		let hold_amount = Amount::try_from("20.0").unwrap();

		account.hold(hold_amount.clone()).unwrap();

		assert_eq!(account.held, hold_amount);
	}

	#[test]
	fn test_release() {
		let client_id = 1;
		let mut account =
			Account::new(client_id, Amount::try_from("100.0").unwrap(), Amount::default(), false);
		let hold_amount = Amount::try_from("20.0").unwrap();

		account.hold(hold_amount.clone()).unwrap();
		account.release(hold_amount.clone()).unwrap();

		assert_eq!(account.held, Amount::default());
	}

	#[test]
	fn test_chargeback() {
		let client_id = 1;
		let mut account = Account::new(
			client_id,
			Amount::try_from("100.0").unwrap(),
			Amount::try_from("20.0").unwrap(),
			false,
		);
		let chargeback_amount = Amount::try_from("20.0").unwrap();

		account.chargeback(chargeback_amount.clone()).unwrap();

		assert_eq!(account.held, Amount::default());
		assert!(account.locked);
	}

	#[test]
	fn test_total() {
		let client_id = 1;
		let account = Account::new(
			client_id,
			Amount::try_from("100.0").unwrap(),
			Amount::try_from("20.0").unwrap(),
			false,
		);

		let total = account.total();

		assert_eq!(total, Amount::try_from("120.0").unwrap());
	}

	#[test]
	fn test_locked() {
		let client_id = 1;
		let mut account = Account::new(
			client_id,
			Amount::try_from("100.0").unwrap(),
			Amount::try_from("20.0").unwrap(),
			true,
		);

		assert_eq!(account.deposit(Amount::try_from("10.0").unwrap()), Err(AccountLocked));
		assert_eq!(account.withdraw(Amount::try_from("10.0").unwrap()), Err(AccountLocked));
		assert_eq!(account.hold(Amount::try_from("10.0").unwrap()), Err(AccountLocked));
		assert_eq!(account.release(Amount::try_from("10.0").unwrap()), Err(AccountLocked));
		assert_eq!(account.chargeback(Amount::try_from("10.0").unwrap()), Err(AccountLocked));
	}
}
