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
///
/// Serialisation is **redacted by design**: a serialised `Pan` carries only `<bin>****<last4>`,
/// never the full number, so persisting/transporting a [`crate::transaction::Transaction`] (audit
/// log, event backbone) cannot leak card data. Deserialisation is therefore lossy — a round-tripped
/// `Pan` holds only the redacted token (and so compares unequal to the original full PAN), which is
/// the intended security posture.
#[derive(Clone, PartialEq, Eq, Hash)]
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

    /// The redacted token safe to persist or transport: `<bin>****<last4>` (or `****<last4>` when
    /// the PAN is too short to have a BIN). Never contains the full PAN.
    #[must_use]
    pub fn redacted(&self) -> String {
        match self.bin() {
            Some(bin) => format!("{}****{}", bin.as_str(), self.last4()),
            None => format!("****{}", self.last4()),
        }
    }
}

impl Serialize for Pan {
    /// Emits only the redacted token — a full PAN is never written to the wire (PCI).
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.redacted())
    }
}

impl<'de> Deserialize<'de> for Pan {
    /// Wraps the stored token as-is; since serialisation is redacted, this never recovers a full PAN.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(String::deserialize(deserializer)?))
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

    #[test]
    fn pan_serialize_never_emits_full_pan() {
        let pan = Pan::new("4111111111111234");
        let json = serde_json::to_string(&pan).unwrap();
        // The full PAN must never reach the wire; only the redacted token does.
        assert!(!json.contains("4111111111111234"));
        assert_eq!(json, "\"411111****1234\"");
        // The redacted token still yields bin + last4 after a round-trip.
        let back: Pan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bin().unwrap().as_str(), "411111");
        assert_eq!(back.last4(), "1234");
    }
}
