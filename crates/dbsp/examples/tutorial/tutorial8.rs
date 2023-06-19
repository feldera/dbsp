use anyhow::Result;
use bincode::{Decode, Encode};
use csv::Reader;
use dbsp::{
    operator::{
        time_series::{RelOffset, RelRange},
        FilterMap,
    },
    CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
};
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
    CollectionHandle<String, (u64, isize)>,
    OutputHandle<OrdIndexedZSet<(String, i32, u8), (isize, u64), isize>>,
)> {
    let (vax_stream, vax_handle) = circuit.add_input_zset::<Record, isize>();
    let (pop_stream, pop_handle) = circuit.add_input_indexed_zset::<String, u64, isize>();
    let subset = vax_stream.filter(|r| {
        r.location == "England"
            || r.location == "Northern Ireland"
            || r.location == "Scotland"
            || r.location == "Wales"
    });
    let monthly_totals = subset
        .index_with(|r| {
            (
                (r.location.clone(), r.date.year(), r.date.month() as u8),
                r.daily_vaccinations.unwrap_or(0),
            )
        })
        .aggregate_linear(|(_l, _y, _m), v| *v as isize);
    let running_monthly_totals = monthly_totals
        .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
        .partitioned_rolling_aggregate_linear(
            |vaxxed| *vaxxed,
            |total| total,
            RelRange::new(RelOffset::Before(u32::MAX), RelOffset::Before(0)),
        );
    let vax_rates = running_monthly_totals
        .map_index(|(l, (date, total))| {
            (
                l.clone(),
                ((date / 12) as i32, (date % 12 + 1) as u8, total.unwrap()),
            )
        })
        .join_index(&pop_stream, |l, (y, m, total), pop| {
            Some(((l.clone(), *y, *m), (*total, *pop)))
        });
    Ok((vax_handle, pop_handle, vax_rates.output()))
}

fn main() -> Result<()> {
    let (circuit, (vax_handle, pop_handle, output_handle)) = RootCircuit::build(build_circuit)?;

    let path = format!(
        "{}/examples/tutorial/vaccinations.csv",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut vax_records = Reader::from_path(path)?
        .deserialize()
        .map(|result| result.map(|record| (record, 1)))
        .collect::<Result<Vec<(Record, isize)>, _>>()?;
    vax_handle.append(&mut vax_records);

    let mut pop_records = vec![
        ("England".into(), (56286961, 1)),
        ("Northern Ireland".into(), (1893667, 1)),
        ("Scotland".into(), (5463300, 1)),
        ("Wales".into(), (3152879, 1)),
    ];
    pop_handle.append(&mut pop_records);

    circuit.step()?;

    output_handle
        .consolidate()
        .iter()
        .for_each(|((l, y, m), (vaxxes, pop), w)| {
            let rate = vaxxes as f64 / pop as f64 * 100.0;
            println!("{l:16} {y}-{m:02}: {vaxxes:9} {pop:8} {rate:6.2}% {w:+}",)
        });
    Ok(())
}
