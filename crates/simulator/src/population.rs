//! A fixed pool of entities that generated transactions draw from, so that velocity and linkage
//! features are meaningful (cards recur, devices are shared across accounts, merchants are reused).

use std::net::{IpAddr, Ipv4Addr};

use domain::{AccountId, DeviceId, MerchantId, Pan};

use crate::rng::Rng;

/// A reusable population of entities. Pools are deliberately different sizes so that, for example,
/// many cards share fewer devices — the structure fraud detection keys on.
#[derive(Debug, Clone)]
pub struct Population {
    /// Card PANs.
    pub pans: Vec<Pan>,
    /// Customer accounts.
    pub accounts: Vec<AccountId>,
    /// Device fingerprints (fewer than cards → sharing).
    pub devices: Vec<DeviceId>,
    /// Merchants (far fewer → reuse).
    pub merchants: Vec<MerchantId>,
    /// Source IPs.
    pub ips: Vec<IpAddr>,
}

impl Population {
    /// Generate a population sized off `size` (the number of cards). Other pools are scaled down so
    /// devices, merchants, and IPs are shared. `size` is clamped to at least 1.
    #[must_use]
    pub fn generate(size: usize, rng: &mut Rng) -> Self {
        let cards = size.max(1);
        let accounts = cards;
        let devices = (cards / 3).max(1);
        let merchants = (cards / 20).max(1);
        let ips = (cards / 2).max(1);

        Self {
            pans: (0..cards).map(|_| gen_pan(rng)).collect(),
            accounts: (0..accounts)
                .map(|i| AccountId::new(format!("acct-{i}")))
                .collect(),
            devices: (0..devices)
                .map(|i| DeviceId::new(format!("dev-{i}")))
                .collect(),
            merchants: (0..merchants)
                .map(|i| MerchantId::new(format!("mrc-{i}")))
                .collect(),
            ips: (0..ips).map(|_| gen_ip(rng)).collect(),
        }
    }
}

/// A 16-digit Visa-like PAN (leading `4`), generated deterministically.
fn gen_pan(rng: &mut Rng) -> Pan {
    // 15 digits after the leading 4; 10^15 fits comfortably in u64.
    let tail = rng.below(1_000_000_000_000_000);
    Pan::new(format!("4{tail:015}"))
}

fn gen_ip(rng: &mut Rng) -> IpAddr {
    let v = rng.next_u64();
    IpAddr::V4(Ipv4Addr::new(
        (v >> 24) as u8,
        (v >> 16) as u8,
        (v >> 8) as u8,
        v as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pools_are_scaled_and_nonempty() {
        let mut rng = Rng::new(1);
        let pop = Population::generate(100, &mut rng);
        assert_eq!(pop.pans.len(), 100);
        assert_eq!(pop.accounts.len(), 100);
        assert_eq!(pop.devices.len(), 33); // 100/3
        assert_eq!(pop.merchants.len(), 5); // 100/20
        assert!(
            pop.devices.len() < pop.pans.len(),
            "devices must be shared across cards"
        );
    }

    #[test]
    fn tiny_size_is_clamped_to_one() {
        let mut rng = Rng::new(1);
        let pop = Population::generate(0, &mut rng);
        assert_eq!(pop.pans.len(), 1);
        assert_eq!(pop.merchants.len(), 1);
    }

    #[test]
    fn pans_are_sixteen_digits_leading_four() {
        let mut rng = Rng::new(5);
        let pop = Population::generate(10, &mut rng);
        for pan in &pop.pans {
            assert_eq!(pan.bin().unwrap().as_str().len(), 6);
        }
    }
}
