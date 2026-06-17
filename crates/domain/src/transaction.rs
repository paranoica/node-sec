//! The transaction — the unit a [`crate::decision::Decision`] is made about.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ids::{AccountId, CounterpartyId, DeviceId, MerchantId, Pan, TransactionId};
use crate::money::Money;

/// The fraud vertical a transaction belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Vertical {
    /// Card / payments.
    Card,
    /// Person-to-person push payment.
    P2p,
    /// Cryptocurrency.
    Crypto,
}

/// The channel a transaction arrived through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Channel {
    /// Card-not-present (e-commerce).
    CardNotPresent,
    /// Card-present (EMV / contactless).
    CardPresent,
    /// P2P push payment.
    P2pPush,
    /// Crypto withdrawal (off-ramp).
    CryptoWithdrawal,
    /// Crypto deposit (on-ramp).
    CryptoDeposit,
}

/// An approximate geolocation for a transaction (from IP geo-lookup or device GPS).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Geo {
    /// ISO 3166-1 alpha-2 country code.
    pub country: String,
    /// Latitude in degrees.
    pub lat: f64,
    /// Longitude in degrees.
    pub lon: f64,
}

impl Geo {
    /// Build a geolocation.
    #[must_use]
    pub fn new(country: impl Into<String>, lat: f64, lon: f64) -> Self {
        Self {
            country: country.into(),
            lat,
            lon,
        }
    }

    /// Great-circle distance to another point in kilometres (haversine).
    #[must_use]
    pub fn distance_km(&self, other: &Geo) -> f64 {
        const EARTH_RADIUS_KM: f64 = 6371.0;
        let (lat1, lat2) = (self.lat.to_radians(), other.lat.to_radians());
        let dlat = (other.lat - self.lat).to_radians();
        let dlon = (other.lon - self.lon).to_radians();
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
    }
}

/// A money-movement event to be scored. Entity references are optional because they vary by
/// vertical — a card payment carries a `pan` + `merchant`; a P2P push carries an `account` +
/// `counterparty`. Build with [`Transaction::new`] plus the `with_*` setters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    /// Stable id of this transaction.
    pub id: TransactionId,
    /// Amount — integer minor units plus a currency.
    pub amount: Money,
    /// Event time (UTC).
    pub occurred_at: OffsetDateTime,
    /// The vertical this transaction belongs to.
    pub vertical: Vertical,
    /// The channel of arrival.
    pub channel: Channel,
    /// The card PAN, for a card transaction.
    pub pan: Option<Pan>,
    /// The originating account, if any.
    pub account: Option<AccountId>,
    /// The device fingerprint, if captured.
    pub device: Option<DeviceId>,
    /// The source IP, if captured.
    pub ip: Option<IpAddr>,
    /// The merchant (card vertical).
    pub merchant: Option<MerchantId>,
    /// The counterparty / payee (P2P, crypto).
    pub counterparty: Option<CounterpartyId>,
    /// Geolocation of the transaction, if known.
    pub geo: Option<Geo>,
    /// Merchant Category Code, if known.
    pub mcc: Option<u32>,
    /// AVS result: `Some(true)` match, `Some(false)` mismatch, `None` not checked.
    pub avs_match: Option<bool>,
    /// CVV result: `Some(true)` match, `Some(false)` mismatch, `None` not checked.
    pub cvv_match: Option<bool>,
}

impl Transaction {
    /// Create a transaction with its mandatory fields; entity references default to `None`.
    #[must_use]
    pub fn new(
        id: TransactionId,
        amount: Money,
        occurred_at: OffsetDateTime,
        vertical: Vertical,
        channel: Channel,
    ) -> Self {
        Self {
            id,
            amount,
            occurred_at,
            vertical,
            channel,
            pan: None,
            account: None,
            device: None,
            ip: None,
            merchant: None,
            counterparty: None,
            geo: None,
            mcc: None,
            avs_match: None,
            cvv_match: None,
        }
    }

    /// Attach a card PAN.
    #[must_use]
    pub fn with_pan(mut self, pan: Pan) -> Self {
        self.pan = Some(pan);
        self
    }

    /// Attach an originating account.
    #[must_use]
    pub fn with_account(mut self, account: AccountId) -> Self {
        self.account = Some(account);
        self
    }

    /// Attach a device fingerprint.
    #[must_use]
    pub fn with_device(mut self, device: DeviceId) -> Self {
        self.device = Some(device);
        self
    }

    /// Attach a source IP.
    #[must_use]
    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// Attach a merchant.
    #[must_use]
    pub fn with_merchant(mut self, merchant: MerchantId) -> Self {
        self.merchant = Some(merchant);
        self
    }

    /// Attach a counterparty / payee.
    #[must_use]
    pub fn with_counterparty(mut self, counterparty: CounterpartyId) -> Self {
        self.counterparty = Some(counterparty);
        self
    }

    /// Attach a geolocation.
    #[must_use]
    pub fn with_geo(mut self, geo: Geo) -> Self {
        self.geo = Some(geo);
        self
    }

    /// Attach a Merchant Category Code.
    #[must_use]
    pub fn with_mcc(mut self, mcc: u32) -> Self {
        self.mcc = Some(mcc);
        self
    }

    /// Attach AVS and CVV check results.
    #[must_use]
    pub fn with_card_checks(mut self, avs_match: bool, cvv_match: bool) -> Self {
        self.avs_match = Some(avs_match);
        self.cvv_match = Some(cvv_match);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::money::Currency;
    use time::macros::datetime;

    fn sample() -> Transaction {
        Transaction::new(
            TransactionId::new("txn-1"),
            Money::from_minor_units(4_999, Currency::Usd),
            datetime!(2026-06-17 12:00 UTC),
            Vertical::Card,
            Channel::CardNotPresent,
        )
    }

    #[test]
    fn builder_sets_fields_and_preserves_amount() {
        let txn = sample()
            .with_pan(Pan::new("4111111111111111"))
            .with_merchant(MerchantId::new("mrc-9"));
        assert_eq!(txn.amount.minor_units(), 4_999);
        assert_eq!(txn.amount.currency(), Currency::Usd);
        assert_eq!(txn.pan.as_ref().unwrap().last4(), "1111");
        assert_eq!(txn.merchant.as_ref().unwrap().as_str(), "mrc-9");
        assert!(txn.counterparty.is_none());
    }

    #[test]
    fn serde_roundtrip() {
        let txn = sample().with_account(AccountId::new("acct-1"));
        let json = serde_json::to_string(&txn).unwrap();
        assert_eq!(serde_json::from_str::<Transaction>(&json).unwrap(), txn);
    }
}
