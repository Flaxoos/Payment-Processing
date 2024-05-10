use rust_decimal::RoundingStrategy;
use rusty_money::iso::{Currency, USD};

pub type ClientId = i16;
pub type TransactionId = i32;

pub const CURRENCY: &Currency = USD;
pub const MAX_DECIMAL_PLACES: u8 = 4;
pub const ROUNDING: RoundingStrategy = RoundingStrategy::MidpointAwayFromZero;
