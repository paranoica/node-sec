//! Money as integer minor units with an explicit currency.
//!
//! Floating-point money is forbidden (`arch:money-integer`): there is intentionally no `f64`
//! constructor or conversion. Amounts are a signed count of the currency's minor unit (cents,
//! pence, …); arithmetic is checked and currency-aware.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A currency, carrying the number of decimal places in its minor unit (`exponent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Currency {
    /// US dollar (2 minor digits — cents).
    Usd,
    /// Euro (2 minor digits).
    Eur,
    /// Pound sterling (2 minor digits — pence).
    Gbp,
    /// Japanese yen (0 minor digits).
    Jpy,
}

impl Currency {
    /// Number of decimal places in the minor unit (e.g. cents → 2, yen → 0).
    #[must_use]
    pub const fn exponent(self) -> u8 {
        match self {
            Currency::Usd | Currency::Eur | Currency::Gbp => 2,
            Currency::Jpy => 0,
        }
    }

    /// ISO-4217 alphabetic code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Eur => "EUR",
            Currency::Gbp => "GBP",
            Currency::Jpy => "JPY",
        }
    }
}

/// A monetary amount: a signed count of a currency's minor unit. No floating-point representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Money {
    minor_units: i64,
    currency: Currency,
}

/// Error from a checked money operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    /// The two operands had different currencies.
    CurrencyMismatch,
    /// The operation overflowed `i64`.
    Overflow,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MoneyError::CurrencyMismatch => f.write_str("currency mismatch"),
            MoneyError::Overflow => f.write_str("monetary overflow"),
        }
    }
}

impl std::error::Error for MoneyError {}

impl Money {
    /// Construct from a raw count of minor units (e.g. cents) and a currency.
    #[must_use]
    pub const fn from_minor_units(minor_units: i64, currency: Currency) -> Self {
        Self {
            minor_units,
            currency,
        }
    }

    /// Construct from whole major units and an integer minor remainder — never a float.
    /// `Money::from_major_minor(12, 34, Currency::Usd)` is \$12.34.
    ///
    /// Returns `None` if `minor` is not a valid remainder for the currency (≥ 10^exponent), or if
    /// the result overflows `i64`. The sign is taken from `major`; for a negative sub-unit amount
    /// use [`Money::from_minor_units`].
    #[must_use]
    pub fn from_major_minor(major: i64, minor: u32, currency: Currency) -> Option<Self> {
        let scale = 10i64.checked_pow(u32::from(currency.exponent()))?;
        if i64::from(minor) >= scale {
            return None;
        }
        let units = major.checked_mul(scale)?;
        let minor_signed = if major < 0 {
            -i64::from(minor)
        } else {
            i64::from(minor)
        };
        units.checked_add(minor_signed).map(|minor_units| Self {
            minor_units,
            currency,
        })
    }

    /// The raw minor-unit count.
    #[must_use]
    pub const fn minor_units(self) -> i64 {
        self.minor_units
    }

    /// The currency.
    #[must_use]
    pub const fn currency(self) -> Currency {
        self.currency
    }

    /// True if the amount is exactly zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.minor_units == 0
    }

    /// Checked addition; errors on currency mismatch or `i64` overflow.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if the currencies differ; [`MoneyError::Overflow`] on overflow.
    pub fn checked_add(self, other: Self) -> Result<Self, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch);
        }
        let minor_units = self
            .minor_units
            .checked_add(other.minor_units)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            minor_units,
            currency: self.currency,
        })
    }

    /// Checked subtraction; errors on currency mismatch or `i64` overflow.
    ///
    /// # Errors
    /// [`MoneyError::CurrencyMismatch`] if the currencies differ; [`MoneyError::Overflow`] on overflow.
    pub fn checked_sub(self, other: Self) -> Result<Self, MoneyError> {
        if self.currency != other.currency {
            return Err(MoneyError::CurrencyMismatch);
        }
        let minor_units = self
            .minor_units
            .checked_sub(other.minor_units)
            .ok_or(MoneyError::Overflow)?;
        Ok(Self {
            minor_units,
            currency: self.currency,
        })
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let exp = u32::from(self.currency.exponent());
        if exp == 0 {
            return write!(f, "{} {}", self.minor_units, self.currency.code());
        }
        let scale = 10u64.pow(exp);
        let sign = if self.minor_units < 0 { "-" } else { "" };
        let abs = self.minor_units.unsigned_abs();
        write!(
            f,
            "{sign}{major}.{minor:0width$} {code}",
            major = abs / scale,
            minor = abs % scale,
            width = exp as usize,
            code = self.currency.code(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_minor_units_roundtrips() {
        let m = Money::from_minor_units(1_234, Currency::Usd);
        assert_eq!(m.minor_units(), 1_234);
        assert_eq!(m.currency(), Currency::Usd);
        assert!(!m.is_zero());
    }

    #[test]
    fn from_major_minor_builds_integer_amount() {
        let m = Money::from_major_minor(12, 34, Currency::Usd).unwrap();
        assert_eq!(m.minor_units(), 1_234);
    }

    #[test]
    fn from_major_minor_rejects_out_of_range_minor() {
        // 99 cents ok, 100 is not a valid minor remainder for a 2-digit currency.
        assert!(Money::from_major_minor(1, 99, Currency::Usd).is_some());
        assert!(Money::from_major_minor(1, 100, Currency::Usd).is_none());
    }

    #[test]
    fn zero_minor_currency_only_accepts_zero_minor() {
        assert_eq!(
            Money::from_major_minor(100, 0, Currency::Jpy)
                .unwrap()
                .minor_units(),
            100
        );
        assert!(Money::from_major_minor(100, 1, Currency::Jpy).is_none());
    }

    #[test]
    fn checked_add_same_currency() {
        let a = Money::from_minor_units(100, Currency::Usd);
        let b = Money::from_minor_units(250, Currency::Usd);
        assert_eq!(a.checked_add(b).unwrap().minor_units(), 350);
    }

    #[test]
    fn checked_add_rejects_currency_mismatch() {
        let a = Money::from_minor_units(100, Currency::Usd);
        let b = Money::from_minor_units(100, Currency::Eur);
        assert_eq!(a.checked_add(b), Err(MoneyError::CurrencyMismatch));
    }

    #[test]
    fn checked_add_detects_overflow() {
        let a = Money::from_minor_units(i64::MAX, Currency::Usd);
        let b = Money::from_minor_units(1, Currency::Usd);
        assert_eq!(a.checked_add(b), Err(MoneyError::Overflow));
    }

    #[test]
    fn display_formats_minor_and_zero_exponent() {
        assert_eq!(
            Money::from_minor_units(1_234, Currency::Usd).to_string(),
            "12.34 USD"
        );
        assert_eq!(
            Money::from_minor_units(5, Currency::Usd).to_string(),
            "0.05 USD"
        );
        assert_eq!(
            Money::from_minor_units(-1_234, Currency::Usd).to_string(),
            "-12.34 USD"
        );
        assert_eq!(
            Money::from_minor_units(100, Currency::Jpy).to_string(),
            "100 JPY"
        );
    }

    #[test]
    fn serde_roundtrip() {
        let m = Money::from_minor_units(999, Currency::Gbp);
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Money>(&json).unwrap(), m);
    }
}
