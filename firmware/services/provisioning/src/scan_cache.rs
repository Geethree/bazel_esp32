//! Bounded, time-limited network sightings for intermittent radio scans.

use heapless::Vec;

use crate::Network;

struct Sighting {
    network: Network,
    seen_ms: u64,
}

pub struct ScanCache<const N: usize> {
    sightings: Vec<Sighting, N>,
    ttl_ms: u64,
}

impl<const N: usize> ScanCache<N> {
    pub const fn new(ttl_ms: u64) -> Self {
        Self {
            sightings: Vec::new(),
            ttl_ms,
        }
    }

    /// Merge one scan and discard networks that have not been seen within the TTL.
    pub fn observe(&mut self, now_ms: u64, networks: impl IntoIterator<Item = Network>) {
        self.sightings
            .retain(|item| now_ms.saturating_sub(item.seen_ms) < self.ttl_ms);

        for network in networks {
            if network.ssid.is_empty() {
                continue;
            }
            if let Some(existing) = self
                .sightings
                .iter_mut()
                .find(|item| item.network.ssid == network.ssid)
            {
                if existing.seen_ms != now_ms || network.rssi > existing.network.rssi {
                    existing.network = network;
                }
                existing.seen_ms = now_ms;
                continue;
            }

            let entry = Sighting {
                network,
                seen_ms: now_ms,
            };
            if !self.sightings.is_full() {
                let _ = self.sightings.push(entry);
            } else if let Some((weakest, _)) = self
                .sightings
                .iter()
                .enumerate()
                .min_by_key(|(_, item)| item.network.rssi)
                && entry.network.rssi > self.sightings[weakest].network.rssi
            {
                self.sightings[weakest] = entry;
            }
        }
    }

    pub fn networks(&self) -> impl Iterator<Item = Network> + '_ {
        self.sightings.iter().map(|item| item.network.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network(ssid: &str, rssi: i8) -> Network {
        Network::new(ssid, rssi, true).unwrap()
    }

    #[test]
    fn keeps_sightings_across_empty_scans_until_ttl() {
        let mut cache = ScanCache::<4>::new(120_000);
        cache.observe(1_000, [network("Porta Amarela", -65)]);
        cache.observe(60_000, []);
        assert_eq!(
            cache.networks().next().unwrap().ssid.as_str(),
            "Porta Amarela"
        );
        cache.observe(121_000, []);
        assert_eq!(cache.networks().count(), 0);
    }

    #[test]
    fn refreshes_sighting_and_keeps_strongest_when_full() {
        let mut cache = ScanCache::<2>::new(120_000);
        cache.observe(1_000, [network("weak", -90), network("strong", -40)]);
        cache.observe(50_000, [network("strong", -45), network("new", -60)]);
        let found: Vec<_, 2> = cache.networks().collect();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|item| item.ssid == "strong"));
        assert!(found.iter().any(|item| item.ssid == "new"));
        cache.observe(121_000, []);
        assert_eq!(cache.networks().count(), 2);
        cache.observe(170_000, []);
        assert_eq!(cache.networks().count(), 0);
    }

    #[test]
    fn picks_strongest_access_point_for_one_ssid_in_each_scan() {
        let mut cache = ScanCache::<2>::new(120_000);
        cache.observe(1_000, [network("mesh", -45), network("mesh", -75)]);
        assert_eq!(cache.networks().next().unwrap().rssi, -45);
        cache.observe(2_000, [network("mesh", -60)]);
        assert_eq!(cache.networks().next().unwrap().rssi, -60);
    }
}
