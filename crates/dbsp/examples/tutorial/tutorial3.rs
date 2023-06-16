use anyhow::Result;
use bincode::{Decode, Encode};
use csv::Reader;
use dbsp::{operator::FilterMap, CollectionHandle, OrdZSet, OutputHandle, RootCircuit, ZSet};
use serde::Deserialize;
use size_of::SizeOf;
use time::Date;

#[derive(
    Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode,
)]
struct Record {
    location: String,
    #[bincode(with_serde)]
    date: Date,
    daily_vaccinations: Option<u64>,
}
fn build_circuit(
    circuit: &mut RootCircuit,
) -> Result<(
    CollectionHandle<Record, isize>,
    OutputHandle<OrdZSet<Record, isize>>,
)> {
    let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
    input_stream.inspect(|records| {
        println!("{}", records.weighted_count());
    });
    let filtered = input_stream.filter(|r| {
        r.location == "England"
            || r.location == "Northern Ireland"
            || r.location == "Scotland"
            || r.location == "Wales"
    });
    Ok((input_handle, filtered.output()))
}

fn main() -> Result<()> {
    // Build circuit.
    let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;

    // Feed data into circuit.
    let path = format!(
        "{}/examples/tutorial/vaccinations.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut input_records = Reader::from_path(path)?
        .deserialize()
        .map(|result| result.map(|record| (record, 1)))
        .collect::<Result<Vec<(Record, isize)>, _>>()?;
    input_handle.append(&mut input_records);

    // Execute circuit.
    circuit.step()?;

    // Read output from circuit.
    println!("{}", output_handle.consolidate().weighted_count());

    Ok(())
}
