use bincode::{Decode, Encode};
use dbsp::circuit::Layout;
use serde::{Deserialize, Serialize};
use size_of::SizeOf;
use time::Date;

#[derive(
    Clone,
    Debug,
    Decode,
    Deserialize,
    Encode,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize,
    SizeOf,
)]
pub struct Record {
    pub location: String,
    #[bincode(with_serde)]
    pub date: Date,
    pub daily_vaccinations: Option<u64>,
}

#[derive(
    Clone,
    Debug,
    Decode,
    Deserialize,
    Encode,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Serialize,
    SizeOf,
)]
pub struct VaxMonthly {
    pub count: u64,
    pub year: i32,
    pub month: u8,
}

#[tarpc::service]
pub trait Circuit {
    async fn init(layout: Layout);
    async fn run(records: Vec<(Record, isize)>) -> Vec<(String, VaxMonthly, isize)>;
}
