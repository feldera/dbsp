//! Generators for the models usd in the Nexmark benchmark suite.
//!
//! Based on the equivalent [Nexmark Flink generator API](https://github.com/nexmark/nexmark/blob/v0.2.0/nexmark-flink/src/main/java/com/github/nexmark/flink/generator).

use self::config::Config;
use crate::model::Event;
use anyhow::{Context, Result};
use bids::CHANNELS_NUMBER;
use cached::SizedCache;
use rand::Rng;
use std::time::SystemTime;

mod auctions;
mod bids;
pub mod config;
mod people;
mod price;
mod strings;

pub struct NexmarkGenerator<R: Rng> {
    /// Configuration to generate events against. Note that it may be replaced
    /// by a call to `splitAtEventId`.
    config: Config,
    rng: R,

    /// The memory cache used when creating bid channels.
    bid_channel_cache: SizedCache<usize, (String, String)>,

    /// Number of events generated by this generator.
    events_count_so_far: u64,

    /// Wallclock time at which we emitted the first event (ms since epoch).
    /// Initialised to the current system time when the first event is
    /// emitted.
    wallclock_base_time: Option<u64>,
}

impl<R: Rng> NexmarkGenerator<R> {
    pub fn new(config: Config, rng: R) -> NexmarkGenerator<R> {
        NexmarkGenerator {
            config,
            rng,
            bid_channel_cache: SizedCache::with_size(CHANNELS_NUMBER),
            events_count_so_far: 0,
            wallclock_base_time: None,
        }
    }

    /// Set the wallclock base time explicitly rather than leaving it
    /// to be set when the first event is generated. Useful when testing.
    pub fn set_wallclock_base_time(&mut self, base_time: u64) {
        self.wallclock_base_time = match self.wallclock_base_time {
            None => Some(base_time),
            Some(t) => {
                panic!(
                    "attempt to set wallclock basetime to {} when already set at {}.",
                    base_time, t
                )
            }
        };
    }

    fn get_next_event_id(&self) -> u64 {
        self.config.first_event_id
            + self
                .config
                .next_adjusted_event_number(self.events_count_so_far)
    }

    pub fn next_event(&mut self) -> Result<NextEvent> {
        if self.wallclock_base_time == None {
            self.wallclock_base_time = Some(
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)?
                    .as_millis()
                    .try_into()?,
            )
        }

        // When, in event time, we should generate the event. Monotonic.
        let event_timestamp = self
            .config
            .timestamp_for_event(self.config.next_event_number(self.events_count_so_far));
        // When, in event time, the event should say it was generated. Depending on
        // outOfOrderGroupSize may have local jitter.
        let adjusted_event_timestamp = self.config.timestamp_for_event(
            self.config
                .next_adjusted_event_number(self.events_count_so_far),
        );
        // The minimum of this and all future adjusted event timestamps. Accounts for
        // jitter in the event timestamp.
        let watermark = self.config.timestamp_for_event(
            self.config
                .next_event_number_for_watermark(self.events_count_so_far),
        );
        // When, in wallclock time, we should emit the event.
        let wallclock_timestamp = self
            .wallclock_base_time
            .context("wallclock_base_time not set")?
            + event_timestamp;

        let (auction_proportion, person_proportion, total_proportion) = (
            self.config.nexmark_config.auction_proportion as u64,
            self.config.nexmark_config.person_proportion as u64,
            self.config.nexmark_config.total_proportion() as u64,
        );

        let new_event_id = self.get_next_event_id();
        let rem = new_event_id % total_proportion;

        let event = if rem < person_proportion {
            Event::NewPerson(self.next_person(new_event_id, adjusted_event_timestamp))
        } else if rem < person_proportion + auction_proportion {
            Event::NewAuction(self.next_auction(
                self.events_count_so_far,
                new_event_id,
                adjusted_event_timestamp,
            )?)
        } else {
            Event::NewBid(self.next_bid(new_event_id, adjusted_event_timestamp))
        };

        self.events_count_so_far += 1;
        Ok(NextEvent {
            wallclock_timestamp,
            event_timestamp,
            event,
            watermark,
        })
    }
}

/// The next event and its various timestamps. Ordered by increasing wallclock
/// timestamp, then (arbitrary but stable) event hash order.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NextEvent {
    /// When, in wallclock time, should this event be emitted?
    pub wallclock_timestamp: u64,

    /// When, in event time, should this event be considered to have occured?
    pub event_timestamp: u64,

    /// The event itself.
    pub event: Event,

    /// The minimum of this and all future event timestamps.
    pub watermark: u64,
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use rand::{rngs::mock::StepRng, thread_rng};

    pub fn make_test_generator() -> NexmarkGenerator<StepRng> {
        NexmarkGenerator::new(Config::default(), StepRng::new(0, 1))
    }

    /// Generates a specified number of next events using the default test
    /// generator.
    pub fn generate_expected_next_events(
        wallclock_base_time: u64,
        num_events: usize,
    ) -> Vec<NextEvent> {
        let mut ng = make_test_generator();
        ng.set_wallclock_base_time(wallclock_base_time);

        (0..num_events).map(|_| ng.next_event().unwrap()).collect()
    }

    #[test]
    fn test_next_event_id() {
        let mut ng = make_test_generator();

        assert_eq!(ng.get_next_event_id(), 0);
        ng.next_event().unwrap();
        assert_eq!(ng.get_next_event_id(), 1);
        ng.next_event().unwrap();
        assert_eq!(ng.get_next_event_id(), 2);
    }

    // Tests the first five expected events without relying on any test
    // helper for the data.
    #[test]
    fn test_next_event() {
        let mut ng = NexmarkGenerator::new(Config::default(), thread_rng());

        // The first event with the default config is the person
        let next_event = ng.next_event().unwrap();

        assert!(
            matches!(next_event.event, Event::NewPerson(_)),
            "got: {:?}, want: Event::NewPerson(_)",
            next_event.event
        );
        assert_eq!(next_event.event_timestamp, 0);

        // The next 3 events with the default config are auctions
        for event_num in 1..=3 {
            let next_event = ng.next_event().unwrap();

            assert!(
                matches!(next_event.event, Event::NewAuction(_)),
                "got: {:?}, want: Event::NewAuction(_)",
                next_event.event
            );
            assert_eq!(next_event.event_timestamp, event_num * 100);
        }

        // And the rest of the events in the first epoch are bids.
        for event_num in 4..=49 {
            let next_event = ng.next_event().unwrap();

            assert!(
                matches!(next_event.event, Event::NewBid(_)),
                "got: {:?}, want: Event::NewBid(_)",
                next_event.event
            );
            assert_eq!(next_event.event_timestamp, event_num * 100);
        }

        // The next epoch begins with another person etc.
        let next_event = ng.next_event().unwrap();

        assert_eq!(next_event.event_timestamp, 50 * 100);
        assert!(
            matches!(next_event.event, Event::NewPerson(_)),
            "got: {:?}, want: Event::NewPerson(_)",
            next_event.event
        );
    }

    // Verifies that the `generate_expected_next_events()` test helper does
    // indeed output predictable results matching the order verified manually in
    // the above `test_next_events` (at least for the first 5 events).  Together
    // with the manual test above of next_events, this ensures the order is
    // correct for external call-sites.
    #[test]
    fn test_generate_expected_next_events() {
        let mut ng = make_test_generator();
        ng.set_wallclock_base_time(1_000_000);

        let expected_events = generate_expected_next_events(1_000_000, 100);

        assert_eq!(
            (0..100)
                .map(|_| ng.next_event().unwrap())
                .collect::<Vec<NextEvent>>(),
            expected_events
        );
    }
}
