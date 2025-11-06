//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{num::NonZeroU64, time::Duration};

use tari_common_types::seeds::cipher_seed;
use tari_ootle_common_types::Epoch;

const BIRTHDAY_GENESIS_FROM_UNIX_EPOCH: Duration = Duration::from_secs(cipher_seed::BIRTHDAY_GENESIS_FROM_UNIX_EPOCH);

#[derive(Debug, Clone, Copy)]
pub struct EpochBirthday {
    /// The duration of an epoch in seconds.
    /// NOTE: actual epoch time may vary depending on network conditions, which could lead to inaccuracies.
    epoch_time_secs: NonZeroU64,
    /// The point in time that represents the first epoch (Epoch(0)). Represented as the number seconds since the
    /// Minotari epoch (see CipherSeed).
    rel_zero_epoch_secs: u64,
}

impl EpochBirthday {
    pub const fn new(epoch_time_secs: NonZeroU64, rel_zero_epoch_secs: u64) -> Self {
        Self {
            epoch_time_secs,
            rel_zero_epoch_secs,
        }
    }

    /// An epoch birthday that will always calculate to epoch zero.
    /// (well until the u64 overflows in 584 billion years...)
    pub const fn far_future() -> Self {
        Self {
            epoch_time_secs: NonZeroU64::new(u64::MAX).unwrap(),
            rel_zero_epoch_secs: 1200,
        }
    }

    pub fn zero_epoch_time_secs(&self) -> u64 {
        self.rel_zero_epoch_secs
    }

    pub fn now_relative_to_zero_epoch(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH + BIRTHDAY_GENESIS_FROM_UNIX_EPOCH)
            .ok()
            .and_then(|t| t.as_secs().checked_sub(self.rel_zero_epoch_secs))
            .unwrap_or_default()
    }

    pub fn calculate_current_epoch(&self) -> Epoch {
        let now = self.now_relative_to_zero_epoch();
        self.calculate_epoch_rel_zero_epoch(now)
    }

    /// Calculate the epoch for a Minotari-relative timestamp in seconds
    pub const fn calculate_epoch_rel_minotari(&self, timestamp_secs: u64) -> Epoch {
        // We use saturating sub, because the zero epoch time can be in the future, in which case we define the birthday
        // epoch as zero
        self.calculate_epoch_rel_zero_epoch(timestamp_secs.saturating_sub(self.rel_zero_epoch_secs))
    }

    /// Calculate the epoch for a given timestamp in seconds relative to the zero epoch time.
    pub const fn calculate_epoch_rel_zero_epoch(&self, timestamp_secs: u64) -> Epoch {
        Epoch(timestamp_secs / self.epoch_time_secs.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_always_calcuates_zero_if_zero_epoch_time_is_in_the_future() {
        let birthday = EpochBirthday::far_future();
        let epoch = birthday.calculate_current_epoch();
        assert_eq!(epoch, Epoch::zero());
        let timestamp_secs = minotari_now() + 3600 * 4;
        let epoch = birthday.calculate_epoch_rel_minotari(timestamp_secs);
        assert_eq!(epoch, Epoch::zero());
        let timestamp_secs = 7200;
        let epoch = birthday.calculate_epoch_rel_zero_epoch(timestamp_secs);
        assert_eq!(epoch, Epoch::zero());
    }

    #[test]
    fn it_calculates_the_current_epoch() {
        let now = minotari_now();
        let expected_epoch = 5;
        let rel_zero_epoch_secs = now - (expected_epoch * 1200);

        let birthday = EpochBirthday::new(1200.try_into().unwrap(), rel_zero_epoch_secs);
        let calculated_epoch = birthday.calculate_current_epoch();
        assert_eq!(calculated_epoch, Epoch(expected_epoch));
    }

    #[test]
    fn it_calculates_the_epoch_relative_to_the_minotari_timestamp() {
        let now = minotari_now();
        let zero_epoch = now + 3600; // zero epoch starts at 1 hour after the minotari epoch

        let birthday = EpochBirthday::new(1200.try_into().unwrap(), zero_epoch);
        let timestamp_secs = now + 3600 * 4; // 4 hours after the minotari epoch = 3 hour after the zero epoch
        let epoch = birthday.calculate_epoch_rel_minotari(timestamp_secs);
        assert_eq!(epoch, Epoch(9));
    }

    #[test]
    fn it_calculates_the_epoch_relative_to_the_zero_epoch() {
        let now = minotari_now();
        let zero_epoch = now + 3600; // zero epoch starts at 1 hour after the minotari epoch

        let birthday = EpochBirthday::new(1200.try_into().unwrap(), zero_epoch);
        let timestamp_secs = 7200; // 2 hours after the zero epoch
        let epoch = birthday.calculate_epoch_rel_zero_epoch(timestamp_secs);
        assert_eq!(epoch, Epoch(6));
    }

    fn minotari_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH + BIRTHDAY_GENESIS_FROM_UNIX_EPOCH)
            .map(|t| t.as_secs())
            .unwrap_or_default()
    }
}
