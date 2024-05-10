use core::fmt;
use std::ops::AddAssign;
use std::ops::SubAssign;

use rusty_money::iso::Currency;
use rusty_money::Money;

use crate::amount::AmountError::{NegativeValue, SubtractToNegative};
use crate::config::CURRENCY;

/// Wrapper for Money, used to enforce positive values and handle deserialization of Money from strings
#[derive(PartialEq, Clone)]
pub struct Amount {
	value: Money<'static, Currency>,
}

pub(crate) type AmountResult = Result<Amount, AmountError>;

#[derive(Debug)]
pub enum AmountError {
	NegativeValue(Money<'static, Currency>),
	SubtractToNegative(Amount, Amount),
}

impl std::fmt::Debug for Amount {
	fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
		f.write_str(self.value.amount().to_string().as_str())
	}
}

impl Default for Amount {
	fn default() -> Self {
		Amount { value: Money::from_str("0.0", CURRENCY).unwrap() }
	}
}

impl Amount {
	pub(crate) fn checked_sub_assign(&mut self, rhs: Amount) -> Result<(), AmountError> {
		if self.value >= rhs.value {
			self.value.sub_assign(rhs.value);
			Ok(())
		} else {
			Err(SubtractToNegative(self.clone(), rhs.clone()))
		}
	}

	pub(crate) fn add_assign(&mut self, rhs: Amount) {
		self.value.add_assign(rhs.value)
	}
}

impl TryFrom<&str> for Amount {
	type Error = AmountError;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		Amount::try_from(Money::from_str(value, CURRENCY).unwrap())
	}
}

impl TryFrom<Money<'static, Currency>> for Amount {
	type Error = AmountError;

	fn try_from(value: Money<'static, Currency>) -> AmountResult {
		if value.is_negative() {
			Err(NegativeValue(value))
		} else {
			Ok(Amount { value })
		}
	}
}

impl Amount {
	pub fn value(&self) -> &Money<'static, Currency> {
		&self.value
	}
}

impl fmt::Display for AmountError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			NegativeValue(money) => write!(f, "Amount cannot be negative: {}", money),
			SubtractToNegative(lhs, rhs) => {
				write!(f, "Subtraction results in negative amount: {} - {}", lhs.value, rhs.value)
			},
		}
	}
}
#[cfg(test)]
mod tests {
	use rust_decimal::prelude::ToPrimitive;

	use super::*;

	#[test]
	fn test_checked_sub_assign() {
		let mut amount1 = Amount::try_from(Money::from_str("10.0", CURRENCY).unwrap()).unwrap();
		let amount2 = Amount::try_from(Money::from_str("5.0", CURRENCY).unwrap()).unwrap();

		amount1.checked_sub_assign(amount2.clone()).unwrap();

		assert_eq!(amount1.value().amount().to_f32().unwrap(), 5.0);
	}

	#[test]
	fn test_add_assign() {
		let mut amount1 = Amount::try_from(Money::from_str("10.0", CURRENCY).unwrap()).unwrap();
		let amount2 = Amount::try_from(Money::from_str("5.0", CURRENCY).unwrap()).unwrap();

		amount1.add_assign(amount2.clone());

		assert_eq!(amount1.value().amount().to_f32().unwrap(), 15.0);
	}

	#[test]
	fn test_try_from_str() {
		let amount = Amount::try_from("20.0").unwrap();

		assert_eq!(amount.value().amount().to_f32().unwrap(), 20.0);
	}

	#[test]
	fn test_try_from_negative_str() {
		let amount = Amount::try_from("-20.0");

		assert!(amount.is_err());
		let error = amount.unwrap_err();
		if let AmountError::NegativeValue(money) = error {
			assert_eq!(money.amount().to_f32().unwrap(), -20.0);
		} else {
			panic!("Unexpected error: {:?}", error);
		}
	}

	#[test]
	fn test_try_from_money() {
		let money = Money::from_str("30.0", CURRENCY).unwrap();
		let amount = Amount::try_from(money.clone()).unwrap();

		assert_eq!(amount.value().amount().to_f32().unwrap(), 30.0);
	}

	#[test]
	fn test_try_from_negative_money() {
		let money = Money::from_str("-30.0", CURRENCY).unwrap();
		let amount = Amount::try_from(money.clone());

		assert!(amount.is_err());
		let error = amount.unwrap_err();
		if let AmountError::NegativeValue(money) = error {
			assert_eq!(money.amount().to_f32().unwrap(), -30.0);
		} else {
			panic!("Unexpected error: {:?}", error);
		}
	}
}
