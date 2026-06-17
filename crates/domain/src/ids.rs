//! Strongly-typed identifiers for the entities a decision reasons over.
//!
//! Newtypes stop accidental mixing (passing a [`DeviceId`] where an [`AccountId`] is expected is a
//! compile error). [`Pan`] is sensitive and redacts itself in `Debug`/`Display`.

use core::fmt;

use serde::{Deserialize, Serialize};

/// Define a transparent `String` newtype id with `new`/`as_str` and `Debug`/`Display`.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(String);

        impl $name {
            /// Wrap a raw identifier string.
            #[must_use]
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// Borrow the underlying string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id!(
    /// Customer account identifier.
    AccountId
);
string_id!(
    /// Device fingerprint identifier.
    DeviceId
);
string_id!(
    /// Merchant identifier.
    MerchantId
);
string_id!(
    /// Counterparty identifier — a payee account (P2P) or an on-chain address (crypto).
    CounterpartyId
);
string_id!(
    /// Transaction identifier.
    TransactionId
);
string_id!(
    /// Bank Identification Number — the issuer prefix of a card PAN.
    Bin
);

/// A card Primary Account Number. **Sensitive**: `Debug` and `Display` are redacted to the last
/// four digits so it never leaks through formatting. In production a PAN should be tokenised
/// upstream; this type is the last line of defence against logging a full PAN.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Pan(String);

impl Pan {
    /// Wrap a raw PAN string.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The issuer BIN (first six characters), if the PAN has at least six.
    #[must_use]
    pub fn bin(&self) -> Option<Bin> {
        let prefix: String = self.0.chars().take(6).collect();
        (prefix.chars().count() == 6).then(|| Bin::new(prefix))
    }

    /// The last four characters (or fewer if shorter) — the only part safe to display.
    #[must_use]
    pub fn last4(&self) -> String {
        let count = self.0.chars().count();
        self.0.chars().skip(count.saturating_sub(4)).collect()
    }
}

impl fmt::Debug for Pan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pan(****{})", self.last4())
    }
}

impl fmt::Display for Pan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "****{}", self.last4())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_id_display_and_debug() {
        let a = AccountId::new("acct-1");
        assert_eq!(a.as_str(), "acct-1");
        assert_eq!(a.to_string(), "acct-1");
        assert_eq!(format!("{a:?}"), "AccountId(\"acct-1\")");
    }

    #[test]
    fn pan_redacts_in_debug_and_display() {
        let pan = Pan::new("4111111111111234");
        // The full PAN must never appear in either representation.
        assert!(!format!("{pan:?}").contains("4111111111111234"));
        assert!(!format!("{pan}").contains("4111111111111234"));
        assert_eq!(format!("{pan:?}"), "Pan(****1234)");
        assert_eq!(pan.to_string(), "****1234");
        assert_eq!(pan.last4(), "1234");
    }

    #[test]
    fn pan_bin_is_first_six() {
        assert_eq!(
            Pan::new("411111222233334444").bin().unwrap().as_str(),
            "411111"
        );
        assert!(Pan::new("123").bin().is_none());
    }
}
