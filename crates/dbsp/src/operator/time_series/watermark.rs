use crate::{
    operator::communication::new_exchange_operators,
    trace::{cursor::Cursor, BatchReader},
    Circuit, NumEntries, RootCircuit, Runtime, Stream,
};
use bincode::{Decode, Encode};
use size_of::SizeOf;
use std::{cmp::max, panic::Location};

impl<B> Stream<RootCircuit, B>
where
    B: BatchReader + Clone + 'static,
{
    /// Compute the watermark of a time series, where the watermark function is
    /// monotonic in event time.  The notion of time here is distinct from the
    /// DBSP logical time and can be modeled using any type that implements
    /// `Ord`.
    ///
    /// Watermark is an attribute of a time series that indicates the latest
    /// timestamp such that no data points with timestamps older than the
    /// watermark should appear in the stream. Every record in the time
    /// series carries watermark information that can be extracted by
    /// applying a user-provided function to it.  The watermark of the time
    /// series is the maximum of watermarks of all its data points.
    ///
    /// This method computes the watermark of a time series assuming that the
    /// watermark function is monotonic in the event time, e.g., `watermark
    /// = event_time - 5s`.  Such watermarks are the most common in practice
    /// and can be computed efficiently by only considering the latest
    /// timestamp in each input batch.   The method takes a stream of batches
    /// indexed by timestamp and outputs a stream of watermarks (scalar
    /// values).  Its output at each timestamp is a scalar (not a Z-set),
    /// computed as the maximum of the previous watermark and the largest
    /// watermark in the new input batch.
    #[track_caller]
    pub fn watermark_monotonic<W, TS>(&self, watermark_func: W) -> Stream<RootCircuit, TS>
    where
        W: Fn(&B::Key) -> TS + 'static,
        TS: Ord + Clone + Default + Decode + Encode + SizeOf + NumEntries + Send + 'static,
    {
        let local_watermark = self.stream_fold(TS::default(), move |old_watermark, batch| {
            let mut cursor = batch.cursor();
            cursor.fast_forward_keys();
            match cursor.get_key() {
                Some(key) => max(old_watermark, watermark_func(key)),
                None => old_watermark,
            }
        });

        if let Some(runtime) = Runtime::runtime() {
            let num_workers = runtime.num_workers();
            if num_workers == 1 {
                return local_watermark;
            }

            let (sender, receiver) = new_exchange_operators(
                &runtime,
                Runtime::worker_index(),
                Some(Location::caller()),
                move |watermark: TS, watermarks: &mut Vec<TS>| {
                    for _ in 0..num_workers {
                        watermarks.push(watermark.clone());
                    }
                },
                |result, watermark| {
                    if &watermark > result {
                        *result = watermark;
                    }
                },
            );

            self.circuit()
                .add_exchange(sender, receiver, &local_watermark)
        } else {
            local_watermark
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Runtime;

    fn test_watermark_monotonic(workers: usize) {
        let mut expected_watermarks = vec![115, 115, 125, 145].into_iter();

        let (mut dbsp, input_handle) = Runtime::init_circuit(workers, move |circuit| {
            let (stream, handle) = circuit.add_input_zset();
            stream
                .watermark_monotonic(|ts| ts + 5)
                .inspect(move |watermark| {
                    if Runtime::worker_index() == 0 {
                        assert_eq!(watermark, &expected_watermarks.next().unwrap());
                    }
                });
            Ok(handle)
        })
        .unwrap();

        input_handle.append(&mut vec![(100, 1), (110, 1), (50, 1)]);
        dbsp.step().unwrap();

        input_handle.append(&mut vec![(90, 1), (90, 1), (50, 1)]);
        dbsp.step().unwrap();

        input_handle.append(&mut vec![(110, 1), (120, 1), (100, 1)]);
        dbsp.step().unwrap();

        input_handle.append(&mut vec![(130, 1), (140, 1), (0, 1)]);
        dbsp.step().unwrap();

        dbsp.kill().unwrap();
    }

    #[test]
    fn test_watermark_monotonic1() {
        test_watermark_monotonic(1);
    }

    #[test]
    fn test_watermark_monotonic4() {
        test_watermark_monotonic(4);
    }
}
