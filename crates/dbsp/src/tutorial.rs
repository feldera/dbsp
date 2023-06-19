//! # Developer tutorial
//!
//! This tutorial aims to be a start at teaching Rust developers how to
//! use DBSP in their projects.
//!
//! All of the programs in this tutorial are provided as examples under
//! `examples/tutorial`.  You can run each of them with, e.g. `cargo run
//! --example tutorial1`.
//!
//! # Table of contents
//!
//! * [Introduction](#introduction)
//! * [Basic](#basics)
//!   * [Input](#input)
//!   * [Execution](#execution)
//!   * [Computation and output](#computation-and-output)
//! * [More sophisticated computation](#more-sophisticated-computation)
//!   * [Aggregation](#aggregation)
//!   * [Rolling aggregation](#rolling-aggregation)
//!   * [Joins](#joins)
//!   * [Finding months with the most
//!     vaccinations](#finding-months-with-the-most-vaccinations)
//!   * [Vaccination rates](#vaccination-rates)
//! * [Incremental computation](#incremental-computation)
//! * [Next steps](#next-steps)
//!
//! # Introduction
//!
//! Computation in DBSP is a two-stage process.  First, create a DBSP "circuit"
//! and fix its computational structure, including its inputs and outputs.
//! Second, any number of times, feed in input changes, tell DBSP to run the
//! circuits, and then read out the output changes.  A skeleton for a DBSP
//! program might look like this (the second and later steps could iterate any
//! number of times):
//!
//! ```
//! fn main() {
//!     // ...build circuit...
//!     // ...feed data into circuit...
//!     // ...execute circuit...
//!     // ...read output from circuit...
//! }
//! ```
//!
//! The following section shows the basics of how to fill in each of these
//! steps.
//!
//! # Basics
//!
//! This section shows off the basics of input, computation, and output.
//! Afterward, we'll show how to do more sophisticated computation.
//!
//! ## Input
//!
//! To process data in DBSP, we need to get data from somewhere.  The
//! `dbsp_adapters` crate in `crates/adapters` implements input and output
//! adapters for a number of formats and transports along with a server that
//! instantiates a DBSP pipeline and adapters based on a user-provided
//! declarative configuration.  In this tutorial we take a different approach,
//! instantiating the pipeline and pushing data to it directly using the Rust
//! API.  Specifically, we will parse some data from a CSV file and bring it
//! into a circuit.
//!
//! Let's work with the [Our World in Data](https://ourworldindata.org/)
//! public-domain dataset on COVID-19 vaccinations, which is available on
//! Github.  Its main data file on vaccinations is `vaccinations.csv`, which
//! contains about 168,000 rows of data.  That's a lot to stick in the DBSP
//! repo, so we've included a subset with data for just a few countries. The
//! full version of the snapshot of the data excerpted here is [freely
//! available](https://github.com/owid/covid-19-data/blob/88ab53d1081ef7651b16212658ea43bd175d572a/public/data/vaccinations/vaccinations.csv)
//! on Github.
//!
//! The vaccination data has 16 columns per row.  We will only look at three of
//! those: `location`, a country name; `date`, a date in the form `yyyy-mm-dd`;
//! and `daily_vaccinations`, the number of vaccinations given on `date` in
//! `location`.  The latter field is sometimes blank.
//!
//! Rust crates have good support for reading this data.  We can combine the
//! `csv` crate to read CSV files with `serde` for deserializing into a `struct`
//! and `time` for parsing the date field. A full program for parsing and
//! printing the data is below and in `tutorial1.rs`:

//! ```rust
//! use anyhow::Result;
//! use csv::Reader;
//! use serde::Deserialize;
//! use time::Date;
//!
//! #[allow(dead_code)]
//! #[derive(Debug, Deserialize)]
//! struct Record {
//!     location: String,
//!     date: Date,
//!     daily_vaccinations: Option<u64>,
//! }
//!
//! fn main() -> Result<()> {
//!     let path = format!(
//!         "{}/examples/tutorial/vaccinations.csv",
//!         env!("CARGO_MANIFEST_DIR")
//!     );
//!     for result in Reader::from_path(path)?.deserialize() {
//!         let record: Record = result?;
//!         println!("{:?}", record);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! If we run this, then it prints the records in `Debug` format.  Here are the
//! first few:
//!
//! ```text
//! Record { location: "England", date: 2021-01-10, daily_vaccinations: None }
//! Record { location: "England", date: 2021-01-11, daily_vaccinations: Some(140441) }
//! Record { location: "England", date: 2021-01-12, daily_vaccinations: Some(164043) }
//! Record { location: "England", date: 2021-01-13, daily_vaccinations: Some(192088) }
//! Record { location: "England", date: 2021-01-14, daily_vaccinations: Some(213978) }
//! ...
//! ```
//!
//! We want to create a DBSP circuit and bring this data into it.  We create a
//! circuit with [`RootCircuit::build`], which creates an empty circuit, calls a
//! callback that we pass it to add input and computation and output to the
//! circuit, and then fixes the form of the circuit and returns the circuit plus
//! anything we returned from our callback.  The code skeleton is like this:
//!
//! ```
//! # use anyhow::Result;
//! # use dbsp::RootCircuit;
//! fn build_circuit(circuit: &mut RootCircuit) -> Result<()> {
//!     // ...populate `circuit` with operators...
//!     Ok((/*handles*/))
//! }
//!
//! fn main() -> Result<()> {
//!     // Build circuit.
//!     let (circuit, (/*handles*/)) = RootCircuit::build(build_circuit)?;
//!
//!     // ...feed data into circuit...
//!     // ...execute circuit...
//!     // ...read output from circuit...
//! #     Ok(())
//! }
//! ```
//!
//! The natural way to bring our data into the circuit is through a "Z-set"
//! ([`ZSet`]) input stream.  A "Z-set" is a set in which each item is
//! associated with an integer weight.  In the context of changes to a data set,
//! positive weights represent insertions and negative weights represent
//! deletions.  The magnitude of the weight represents a count, so that a weight
//! of 1 represents an insertion of a single copy of a record, 2 represents two
//! copies, and so on, and similarly for negative weights and deletions.  Thus,
//! a Z-set represents changes to a multiset.
//!
//! We create the Z-set input stream inside `build_circuit` using
//! [`RootCircuit::add_input_zset`], which returns a [`Stream`] for further use
//! in `build_circuit` and a [`CollectionHandle`] for `main` to use to feed in
//! data.  Our skeleton fills in as shown below.  We're jumping the gun a bit by
//! adding a call to [`inspect`](Stream::inspect) on the `Stream`.  This method
//! calls a closure on each batch of data that passes through; we're having it
//! print the total weight in our Z-set just to demonstrate that something is
//! happening:
//!
//! ```rust
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use dbsp::{CollectionHandle, RootCircuit, ZSet};
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! fn build_circuit(circuit: &mut RootCircuit) -> Result<CollectionHandle<Record, isize>> {
//!     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//!     input_stream.inspect(|records| {
//!         println!("{}", records.weighted_count());
//!     });
//!     // ...populate `circuit` with more operators...
//!     Ok(input_handle)
//! }
//!
//! fn main() -> Result<()> {
//!     // Build circuit.
//!     let (circuit, input_handle) = RootCircuit::build(build_circuit)?;
//!
//!     // ...feed data into circuit...
//!     // ...execute circuit...
//!     // ...read output from circuit...
//! #   Ok(())
//! }
//! ```
//!
//! The best way to feed the records into `input_handle` is to collect them into
//! a `Vec<(Record, isize)>`, where `isize` is the Z-set weight.  All the
//! weights can be 1, since we are inserting each of them.  We feed them in with
//! [`CollectionHandle::append`].  So, we can fill in `// ...feed data into
//! circuit...` with:
//!
//! ```rust
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{CollectionHandle, RootCircuit, ZSet};
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! # fn build_circuit(circuit: &mut RootCircuit) -> Result<CollectionHandle<Record, isize>> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     input_stream.inspect(|records| {
//! #         println!("{}", records.weighted_count());
//! #     });
//! #     // ...populate `circuit` with more operators...
//! #     Ok(input_handle)
//! # }
//! #
//! # fn main() -> Result<()> {
//! #     // Build circuit.
//! #     let (circuit, input_handle) = RootCircuit::build(build_circuit)?;
//! #
//!     // Feed data into circuit.
//!     let path = format!(
//!         "{}/examples/tutorial/vaccinations.csv",
//!         env!("CARGO_MANIFEST_DIR")
//!     );
//!     let mut input_records = Reader::from_path(path)?
//!         .deserialize()
//!         .map(|result| result.map(|record| (record, 1)))
//!         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//!     input_handle.append(&mut input_records);
//!
//! #     // Execute circuit.
//! #     circuit.step()?;
//! #
//! #     // ...read output from circuit...
//! #     Ok(())
//! # }
//! ```
//!
//! The compiler will point out a problem: `Record` lacks several traits
//! required for the record type of the "Z-sets".  We can derive all of them.
//! The `Decode` and `Encode` features need a little help from `bincode` for the
//! member of type `Date`:
//!
//! ```
//! use bincode::{Decode, Encode};
//! use serde::Deserialize;
//! use size_of::SizeOf;
//! use time::Date;
//!
//! #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! ```
//!
//! ## Execution
//!
//! Our program now builds a circuit and feeds data into it.  To execute it, we
//! just replace `// ...execute circuit...` with a call to
//! [`CircuitHandle::step`]:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{CollectionHandle, RootCircuit, ZSet};
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! # fn build_circuit(circuit: &mut RootCircuit) -> Result<CollectionHandle<Record, isize>> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     input_stream.inspect(|records| {
//! #         println!("{}", records.weighted_count());
//! #     });
//! #     // ...populate `circuit` with more operators...
//! #     Ok(input_handle)
//! # }
//! #
//! # fn main() -> Result<()> {
//! #     // Build circuit.
//! #     let (circuit, input_handle) = RootCircuit::build(build_circuit)?;
//! #
//! #     // Feed data into circuit.
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//!     // Execute circuit.
//!     circuit.step()?;
//! #
//! #     // ...read output from circuit...
//! #     Ok(())
//! # }
//! ```
//!
//! Now, if you run our program, with `cargo run --example tutorial2`, it prints
//! `3961`, the number of records in `vaccinations.csv`.  That's because our
//! program reads an entire CSV file and feeds it as input in a single step.
//! That means that running for more steps wouldn't make a difference.  That's
//! not a normal use case for DBSP but, arguably, it's a reasonable setup for a
//! tutorial.
//!
//! ## Computation and output
//!
//! We haven't done any computation inside the circuit, nor have we brought
//! output back out of the circuit yet.  Let's add both of those to our
//! skeleton.
//!
//! Let's do just enough computation to demonstrate the concept.  Suppose we
//! want to pick out a subset of the records.  We can use the [`FilterMap`]
//! trait implemented for [`Stream`] to do that.  For example, we can take just
//! the records for locations in the United Kingdom:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{operator::FilterMap, CollectionHandle, OrdZSet, OutputHandle, RootCircuit, ZSet};
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! # fn build_circuit(
//! #     circuit: &mut RootCircuit,
//! # ) -> Result<(
//! #     CollectionHandle<Record, isize>,
//! #     OutputHandle<OrdZSet<Record, isize>>,
//! # )> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     input_stream.inspect(|records| {
//! #         println!("{}", records.weighted_count());
//! #     });
//!     let filtered = input_stream.filter(|r| {
//!         r.location == "England"
//!             || r.location == "Northern Ireland"
//!             || r.location == "Scotland"
//!             || r.location == "Wales"
//!     });
//! #     Ok((input_handle, filtered.output()))
//! # }
//! #
//! # fn main() -> Result<()> {
//! #     // Build circuit.
//! #     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//! #
//! #     // Feed data into circuit.
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     // Execute circuit.
//! #     circuit.step()?;
//! #
//! #     // Read output from circuit.
//! #     println!("{}", output_handle.consolidate().weighted_count());
//! #
//! #     Ok(())
//! # }
//! ```
//!
//! We could call `inspect` again to print the results.  Instead, let's bring
//! the results out of the computation into `main` and print them there.  That's
//! just a matter of calling [`Stream::output`], which returns [`OutputHandle`]
//! to return to `main`, which can then read out the data after each step.  Our
//! `build_circuit` then looks like this:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{operator::FilterMap, CollectionHandle, OrdZSet, OutputHandle, RootCircuit, ZSet};
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! fn build_circuit(
//!     circuit: &mut RootCircuit,
//! ) -> Result<(
//!     CollectionHandle<Record, isize>,
//!     OutputHandle<OrdZSet<Record, isize>>,
//! )> {
//!     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//!     input_stream.inspect(|records| {
//!         println!("{}", records.weighted_count());
//!     });
//!     let filtered = input_stream.filter(|r| {
//!         r.location == "England"
//!             || r.location == "Northern Ireland"
//!             || r.location == "Scotland"
//!             || r.location == "Wales"
//!     });
//!     Ok((input_handle, filtered.output()))
//! }
//! #
//! # fn main() -> Result<()> {
//! #     // Build circuit.
//! #     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//! #
//! #     // Feed data into circuit.
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     // Execute circuit.
//! #     circuit.step()?;
//! #
//! #     // Read output from circuit.
//! #     println!("{}", output_handle.consolidate().weighted_count());
//! #
//! #     Ok(())
//! # }
//! ```
//!
//! Back in `main`, we need to update the call to [`RootCircuit::build`] so that
//! we save the new `output_handle`.  Then, after we feed in input and execute
//! the circuit, we can read the output.  For general kinds of output, it can be
//! a little tricky using `OutputHandle`, because it supports multithreaded DBSP
//! runtimes that produce one output per thread.  For Z-set output, one can just
//! call its [`consolidate`](`OutputHandle::consolidate`) method, which
//! internally merges the multiple outputs if multiple threads are in use.  To
//! print the number of records, we can just do the following:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{operator::FilterMap, CollectionHandle, OrdZSet, OutputHandle, RootCircuit, ZSet};
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! # fn build_circuit(
//! #     circuit: &mut RootCircuit,
//! # ) -> Result<(
//! #     CollectionHandle<Record, isize>,
//! #     OutputHandle<OrdZSet<Record, isize>>,
//! # )> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     input_stream.inspect(|records| {
//! #         println!("{}", records.weighted_count());
//! #     });
//! #     let filtered = input_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     Ok((input_handle, filtered.output()))
//! # }
//! #
//! # fn main() -> Result<()> {
//!     // Build circuit.
//!     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//! #
//! #     // Feed data into circuit.
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     // Execute circuit.
//! #     circuit.step()?;
//! #
//!     // ...unchanged code to feed data into circuit and execute circuit...
//!
//!     // Read output from circuit.
//!     println!("{}", output_handle.consolidate().weighted_count());
//! #
//! #     Ok(())
//! # }
//! ```
//!
//! Now, if we run it, it prints `3961`, as before, followed by `3083`.  The
//! latter is from the `println!` in `main` and shows that we did select a
//! subset of the 3,961 total records.
//!
//! The full program is in `tutorial3.rs`.
//!
//! # More sophisticated computation
//!
//! Our program only does trivial computation, but DBSP supports much more
//! sophistication.  Let's look at some of what it can do.
//!
//! ## Aggregation
//!
//! 3,083 records is lot.  There's so much because we've got years of daily
//! data.  Let's aggregate daily vaccinations into months, to get monthly
//! vaccinations.  DBSP has several forms of aggregation.  All of them work with
//! "indexed Z-sets" ([`IndexedZSet`]), which are Z-sets of key-value pairs,
//! that is, they associate key-value pairs with weights. Aggregation happens
//! across records with the same key.
//!
//! We will do the equivalent of the following SQL:
//!
//! ```text
//! SELECT SUM(daily_vaccinations) FROM vaccinations GROUP BY location, year, month.
//! ```
//!
//! where `year` and `month` are derived from `date`.
//!
//! To aggregate daily vaccinations over months by location, we need to
//! transform our Z-set into an indexed Z-set where the key (the index) has the
//! form `(location, year, month)` and the value is daily vaccinations (we could
//! keep the whole record but we'd just throw away most of it later).
//! To do this, we call [`Stream::index_with`], passing in a function that maps
//! a record into a key-value tuple:
//!
//! ```ignore
//!     let monthly_totals = subset
//!         .index_with(|r| {
//!             (
//!                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//!                 r.daily_vaccinations.unwrap_or(0),
//!             )
//!         })
//! ```
//!
//! We need to clone the location because it is a `String` that the records
//! incorporate by value.
//!
//! Then we can call [`Stream::aggregate_linear`], the simplest form of
//! aggregation in DBSP, to sum across months.  This function sums the output of
//! a function of key-value pairs across each unique key.  To get monthly
//! vaccinations, we just sum the values from our indexed Z-set (we have to
//! convert to `isize` because aggregation implicitly multiplies by record
//! weights):
//!
//! ```ignore
//!         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! ```
//!
//! We output the indexed Z-set as before, and then in `main` print it record by
//! record:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::FilterMap, CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! fn build_circuit(
//!     circuit: &mut RootCircuit,
//! ) -> Result<(
//!     CollectionHandle<Record, isize>,
//!     OutputHandle<OrdIndexedZSet<(String, i32, u8), isize, isize>>,
//! )> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let subset = input_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//!     // ...
//!     Ok((input_handle, monthly_totals.output()))
//! }
//!
//! fn main() -> Result<()> {
//! #     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//! #
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     circuit.step()?;
//! #
//!     // ...
//!     output_handle
//!         .consolidate()
//!         .iter()
//!         .for_each(|((l, y, m), sum, w)| println!("{l:16} {y}-{m:02} {sum:10}: {w:+}"));
//!
//!     Ok(())
//! }
//! ```
//!
//! The output looks like the following. The `+1`s are the Z-set weights.  They
//! show that each record represents an insertion of a single row:
//!
//! ```text
//! England          2021-01    5600174: +1
//! England          2021-02    9377418: +1
//! England          2021-03   11861175: +1
//! England          2021-04   11288945: +1
//! England          2021-05   13772946: +1
//! England          2021-06   10944915: +1
//! ...
//! Northern Ireland 2021-01     150315: +1
//! Northern Ireland 2021-02     317074: +1
//! ...
//! Wales            2023-01      33838: +1
//! Wales            2023-02      17098: +1
//! Wales            2023-03       8776: +1
//! ```
//!
//! The full program is in `tutorial4.rs`.
//!
//! ### Rolling aggregation
//!
//! By using a "moving average" to average recent data,
//! we can obtain a dataset with less noise due to variation from month
//! to month.   DBSP
//! provides [`Stream::partitioned_rolling_average`] for this purpose.  To
//! use it, we have to map our indexed Z-set into a "partitioned indexed
//! Z-set" ([`PartitionedIndexedZSet`]).
//! This is just an indexed Z-set in which the key is the "partition"
//! within which averaging occurs (for us, this is the country), and the
//! value is a tuple of a "timestamp" and a value.  DBSP uses the
//! timestamp component, which must have an unsigned integer type, to
//! define the window.  We can map our `(country, year, month)` key and
//! `vaccinations` value into a `country` partition, `date` timestamp, and
//! `vaccinations` value like this:
//!
//! ```ignore
//!     let moving_averages = monthly_totals
//!         .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//! ```
//!
//! Once we've done that, computing the moving average is easy.  Here's how we
//! get the average of the current month and the two preceding months (when
//! they're in the data set):
//!
//! ```ignore
//!         .partitioned_rolling_average(RelRange::new(RelOffset::Before(2), RelOffset::Before(0)))
//! ```
//!
//! [`partitioned_rolling_average`](`Stream::partitioned_rolling_average`)
//! returns a partitioned Z-set with the same type as its input except that the
//! value type has an `Option` wrapped around it.  In our case, for example, the
//! input value type was `isize`, so the output value type is `Option<isize>`.
//! The output for a given row is `None` if there are no rows in the window,
//! which can only happen if the range passed in does not include the 0 relative
//! offset (i.e. the current row).  Ours does include 0, so `None` will never
//! occur in our output.
//!
//! Let's re-map to recover year and month from the timestamp that we made and
//! to strip off the `Option`:
//!
//! ```ignore
//!         .map_index(|(l, (date, avg))| ((l.clone(), date / 12, date % 12 + 1), avg.unwrap()));
//! ```
//!
//! If we adjust the `build_circuit` return type and return value, like shown
//! below, the existing code in `main` will print it just fine.
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::{
//! #         time_series::{RelOffset, RelRange},
//! #         FilterMap,
//! #     },
//! #     CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! fn build_circuit(
//!     circuit: &mut RootCircuit,
//! ) -> Result<(
//!     CollectionHandle<Record, isize>,
//!     OutputHandle<OrdIndexedZSet<(String, u32, u32), isize, isize>>,
//! )> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let subset = input_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let moving_averages = monthly_totals
//! #         .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//! #         .partitioned_rolling_average(RelRange::new(RelOffset::Before(2), RelOffset::Before(0)))
//! #         .map_index(|(l, (date, avg))| ((l.clone(), date / 12, date % 12 + 1), avg.unwrap()));
//!     // ...
//!     Ok((input_handle, moving_averages.output()))
//! }
//!
//! # fn main() -> Result<()> {
//! #     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//! #
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     circuit.step()?;
//! #
//! #     output_handle
//! #         .consolidate()
//! #         .iter()
//! #         .for_each(|((l, y, m), sum, w)| println!("{l:16} {y}-{m:02} {sum:10}: {w:+}"));
//! #
//! #     Ok(())
//! # }
//! ```
//!
//! The output looks like this (you can verify that the second row is the
//! average of the first two rows in the previous output, and so on):
//!
//! ```text
//! England          2021-01    5600174: +1
//! England          2021-02    7488796: +1
//! England          2021-03    8946255: +1
//! England          2021-04   10842512: +1
//! England          2021-05   12307688: +1
//! England          2021-06   12002268: +1
//! ...
//! Northern Ireland 2021-01     150315: +1
//! Northern Ireland 2021-02     233694: +1
//! ...
//! Wales            2021-01     295057: +1
//! Wales            2021-02     458273: +1
//! Wales            2021-03     584463: +1
//! ```
//!
//! The whole program is in `tutorial5.rs`.
//!
//! ### Joins
//!
//! Suppose we want both the current month's vaccination count and the moving
//! average together. With enough work, we could get them with just
//! aggregation by writing our own "aggregator" ([`Aggregator`]).  It's a
//! little easier to do a join, and it gives us a chance to show how to do that.
//! Both our monthly vaccination counts and our moving averages are indexed
//! Z-sets with the same key type.
//!
//! The first step is to take the code we've written so far and change the final
//! [`map_index`](`FilterMap::map_index`) on `moving_averages` to include a
//! couple of casts, so that `monthly_totals` and `moving_averages` have exactly
//! the same key type (both `(String, i32, u8)`).  The new call looks like this;
//! only the `as <type>` parts are new:
//!
//! ```ignore
//!         .map_index(|(l, (date, avg))| {
//!             (
//!                 (l.clone(), (date / 12) as i32, (date % 12 + 1) as u8),
//!                 avg.unwrap(),
//!             )
//!         });
//! ```
//!
//! Then we can use [`join_index`](`Stream::join_index`) to do the join and
//! [`inspect`](`Stream::inspect`) to print the results, as shown below.
//! Besides the streams to join, `join_index` take a closure, which it calls for
//! every pair of records with equal keys in the streams, passing in the common
//! key and each stream's value.  The closure must return an iterable collection
//! of output key-value pair tuples.  By returning a collection, the join can
//! output any number of output records per input pairing.
//!
//! We want to output a single record per input pair.  The Rust standard library
//! [`Option`] type's `Some` variant is an iterable collection that has exactly
//! one value, so it's convenient for this purpose:
//!
//! ```ignore
//!     let joined = monthly_totals.join_index(&moving_averages, |(l, y, m), cur, avg| {
//!         Some(((l.clone(), *y, *m), (*cur, *avg)))
//!     });
//!  ```
//!
//! We need to adjust the `build_circuit` return type and value and make `main`
//! print the new kind of output:
//! ```ignore
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::{
//! #         time_series::{RelOffset, RelRange},
//! #         FilterMap,
//! #     },
//! #     CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! fn build_circuit(
//!     circuit: &mut RootCircuit,
//! ) -> Result<(
//!     CollectionHandle<Record, isize>,
//!     OutputHandle<OrdIndexedZSet<(String, i32, u8), (isize, isize), isize>>,
//! )> {
//!     let (input_stream, input_handle) = circuit.add_input_zset::<Record,
//! isize>();
//!
//!     // ...
//!
//! #     let subset = input_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let moving_averages = monthly_totals
//! #         .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//! #         .partitioned_rolling_average(RelRange::new(RelOffset::Before(2), RelOffset::Before(0)))
//! #         .map_index(|(l, (date, avg))| {
//! #             (
//! #                 (l.clone(), (date / 12) as i32, (date % 12 + 1) as u8),
//! #                 avg.unwrap(),
//! #             )
//! #         });
//! #     let joined = monthly_totals.join_index(&moving_averages, |(l, y, m), cur, avg| {
//! #         Some(((l.clone(), *y, *m), (*cur, *avg)))
//! #     });
//!     Ok((input_handle, joined.output()))
//! }
//!
//! fn main() -> Result<()> {
//!     let (circuit, (input_handle, output_handle)) =
//! RootCircuit::build(build_circuit)?;
//!
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     circuit.step()?;
//! #
//!     // ...
//!
//!     output_handle
//!         .consolidate()
//!         .iter()
//!         .for_each(|((l, y, m), (cur, avg), w)| {
//!             println!("{l:16} {y}-{m:02} {cur:10} {avg:10}: {w:+}")
//!         });
//!     Ok(())
//! }
//! ```
//! 
//! The whole program is in `tutorial6.rs`.  If we run it, it prints both per-month
//! vaccination numbers and 3-month moving averages:
//!  
//!  ```text
//! England          2021-01    5600174    5600174: +1
//! England          2021-02    9377418    7488796: +1
//! England          2021-03   11861175    8946255: +1
//! England          2021-04   11288945   10842512: +1
//! England          2021-05   13772946   12307688: +1
//! England          2021-06   10944915   12002268: +1
//! ...
//! ```
//!
//! ### Finding months with the most vaccinations
//!
//! Suppose we want to find the months when the most vaccinations occurred in
//! each country.  [`Stream`] has a ready-made method [`Stream::topk_desc`] for
//! this, which we simply pass the number of top records to keep per group.  (If
//! we only want the greatest value, rather than the top-`k` for `k > 1`, then
//! [`Stream::aggregate`] with the [`Max`] aggregator also works.)
//!
//! There is one tricky part.  To use [`topk_desc`](`Stream::topk_desc`), we
//! must re-index our data so that the country is the key (used for grouping)
//! and the number of vaccinations is the value. But, if we do that in the most
//! obvious way, we end up with just the number of vaccinations as the result,
//! whereas we probably want to know the year and month that that number of
//! occurred as well.
//!
//! One way to recover the year and month is to join against the original data.
//! We can do it without another join by defining a type that is ordered by
//! vaccination count but also contains the year and month.  Taking advantage of
//! how Rust derives [`Ord`] lexicographically, that's as simple as:
//!
//! ```rust
//! # use bincode::{Decode, Encode};
//! # use size_of::SizeOf;
//!
//! # #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! struct VaxMonthly {
//!     count: u64,
//!     year: i32,
//!     month: u8,
//! }
//! ```
//!
//! We can transform our monthly totals from a `(country, year, month)` key and
//! `vaccinations` value in `country` value and `VaxMonthly` value with a call
//! to [`map_index`)(FilterMap::map_index), then just call
//! [`topk_desc`](`Stream::topk_desc`):
//!
//! ```ignore
//!     let most_vax = monthly_totals
//!         .map_index(|((l, y, m), sum)| {
//!             (
//!                 l.clone(),
//!                 VaxMonthly {
//!                     count: *sum as u64,
//!                     year: *y,
//!                     month: *m,
//!                 },
//!             )
//!         })
//!         .topk_desc(3);
//! ```
//!
//! Then we just adjust `build_circuit` return type and value and print the new
//! output type in `main`:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::FilterMap, CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! # #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct VaxMonthly {
//! #     count: u64,
//! #     year: i32,
//! #     month: u8,
//! # }
//! #
//! fn build_circuit(
//!     circuit: &mut RootCircuit,
//! ) -> Result<(
//!     CollectionHandle<Record, isize>,
//!     OutputHandle<OrdIndexedZSet<String, VaxMonthly, isize>>,
//! )> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let subset = input_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let most_vax = monthly_totals
//! #         .map_index(|((l, y, m), sum)| {
//! #             (
//! #                 l.clone(),
//! #                 VaxMonthly {
//! #                     count: *sum as u64,
//! #                     year: *y,
//! #                     month: *m,
//! #                 },
//! #             )
//! #         })
//! #         .topk_desc(3);
//!     // ...
//!     Ok((input_handle, most_vax.output()))
//! }
//!
//! fn main() -> Result<()> {
//! #     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//! #
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut input_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     input_handle.append(&mut input_records);
//! #
//! #     circuit.step()?;
//! #
//!     // ...
//!     output_handle
//!         .consolidate()
//!         .iter()
//!         .for_each(|(l, VaxMonthly { count, year, month }, w)| {
//!             println!("{l:16} {year}-{month:02} {count:10}: {w:+}")
//!         });
//!     Ok(())
//! }
//! ```
//!
//! The complete program is in `tutorial7.rs`.  When we run it, it outputs the
//! following.  The output is sorted in increasing order, which might be a bit
//! surprising, but that's because DBSP Z-sets always iterate in that order.  If
//! it's important to produce output in another order, one could define custom
//! [`Ord`] and [`PartialOrd`] on `VaxMonthly`:
//!
//! ```text
//! England          2021-03   11861175: +1
//! England          2021-05   13772946: +1
//! England          2021-12   14801300: +1
//! Northern Ireland 2021-05     394047: +1
//! Northern Ireland 2021-04     436870: +1
//! Northern Ireland 2021-12     489059: +1
//! Scotland         2021-06    1244155: +1
//! Scotland         2021-05    1272194: +1
//! Scotland         2021-12    1388549: +1
//! Wales            2021-04     707714: +1
//! Wales            2021-12     822945: +1
//! Wales            2021-03     836844: +1
//! ```
//!
//! ## Vaccination rates
//!
//! Suppose we want to compare the population in each country with the
//! number of vaccinations given, that is, to calculate a vaccination
//! rate.  Our input data contains a `total_vaccinations_per_hundred`
//! column that reports this information, but let's calculate it ourselves
//! to demonstrate how it might be done in DBSP.
//!
//! We need to know the population in each country.  Let's add a new input
//! source for population by country.  Since that's naturally a set of key-value
//! pairs, we'll make it an indexed Z-set input source instead of a plain Z-set.
//! The first step is to make our circuit builder construct the source and
//! return its handle.  At the same time, we can prepare for our output to be an
//! indexed Z-set with `(location, year, month)` key and `(vaxxes, population)`
//! value:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::{
//! #         time_series::{RelOffset, RelRange},
//! #         FilterMap,
//! #     },
//! #     CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! fn build_circuit(
//!     circuit: &mut RootCircuit,
//! ) -> Result<(
//!     CollectionHandle<Record, isize>,
//!     CollectionHandle<String, (u64, isize)>,
//!     OutputHandle<OrdIndexedZSet<(String, i32, u8), (isize, u64), isize>>,
//! )> {
//!     let (vax_stream, vax_handle) = circuit.add_input_zset::<Record, isize>();
//!     let (pop_stream, pop_handle) = circuit.add_input_indexed_zset::<String, u64, isize>();
//! #     let subset = vax_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let running_monthly_totals = monthly_totals
//! #         .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//! #         .partitioned_rolling_aggregate_linear(
//! #             |vaxxed| *vaxxed,
//! #             |total| total,
//! #             RelRange::new(RelOffset::Before(u32::MAX), RelOffset::Before(0)),
//! #         );
//! #     let vax_rates = running_monthly_totals
//! #         .map_index(|(l, (date, total))| {
//! #             (
//! #                 l.clone(),
//! #                 ((date / 12) as i32, (date % 12 + 1) as u8, total.unwrap()),
//! #             )
//! #         })
//! #         .join_index(&pop_stream, |l, (y, m, total), pop| {
//! #             Some(((l.clone(), *y, *m), (*total, *pop)))
//! #         });
//!     // ...
//!     Ok((vax_handle, pop_handle, vax_rates.output()))
//! }
//!
//! # fn main() -> Result<()> {
//! #     let (circuit, (vax_handle, pop_handle, output_handle)) =
//! #         RootCircuit::build(build_circuit)?;
//! #
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut vax_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     vax_handle.append(&mut vax_records);
//! #
//! #     let mut pop_records = vec![
//! #         ("England".into(), (56286961, 1)),
//! #         ("Northern Ireland".into(), (1893667, 1)),
//! #         ("Scotland".into(), (5463300, 1)),
//! #         ("Wales".into(), (3152879, 1)),
//! #     ];
//! #     pop_handle.append(&mut pop_records);
//! #
//! #     circuit.step()?;
//! #
//! #     output_handle
//! #         .consolidate()
//! #         .iter()
//! #         .for_each(|((l, y, m), (vaxxes, pop), w)| {
//! #             let rate = vaxxes as f64 / pop as f64 * 100.0;
//! #             println!("{l:16} {y}-{m:02}: {vaxxes:9} {pop:8} {rate:6.2}% {w:+}",)
//! #         });
//! #     Ok(())
//! # }
//! ```
//!
//! The code in `main` needs to receive the additional handle and feed data into
//! it.  Let's feed in fixed data this time:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::{
//! #         time_series::{RelOffset, RelRange},
//! #         FilterMap,
//! #     },
//! #     CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! # fn build_circuit(
//! #     circuit: &mut RootCircuit,
//! # ) -> Result<(
//! #     CollectionHandle<Record, isize>,
//! #     CollectionHandle<String, (u64, isize)>,
//! #     OutputHandle<OrdIndexedZSet<(String, i32, u8), (isize, u64), isize>>,
//! # )> {
//! #     let (vax_stream, vax_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let (pop_stream, pop_handle) = circuit.add_input_indexed_zset::<String, u64, isize>();
//! #     let subset = vax_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let running_monthly_totals = monthly_totals
//! #         .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//! #         .partitioned_rolling_aggregate_linear(
//! #             |vaxxed| *vaxxed,
//! #             |total| total,
//! #             RelRange::new(RelOffset::Before(u32::MAX), RelOffset::Before(0)),
//! #         );
//! #     let vax_rates = running_monthly_totals
//! #         .map_index(|(l, (date, total))| {
//! #             (
//! #                 l.clone(),
//! #                 ((date / 12) as i32, (date % 12 + 1) as u8, total.unwrap()),
//! #             )
//! #         })
//! #         .join_index(&pop_stream, |l, (y, m, total), pop| {
//! #             Some(((l.clone(), *y, *m), (*total, *pop)))
//! #         });
//! #     Ok((vax_handle, pop_handle, vax_rates.output()))
//! # }
//! #
//! fn main() -> Result<()> {
//!     let (circuit, (vax_handle, pop_handle, output_handle)) =
//!         RootCircuit::build(build_circuit)?;
//! #
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut vax_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     vax_handle.append(&mut vax_records);
//! #
//!     // ...
//!     let mut pop_records = vec![
//!         ("England".into(), (56286961, 1)),
//!         ("Northern Ireland".into(), (1893667, 1)),
//!         ("Scotland".into(), (5463300, 1)),
//!         ("Wales".into(), (3152879, 1)),
//!     ];
//!     pop_handle.append(&mut pop_records);
//!     // ...
//! #
//! #     circuit.step()?;
//! #
//! #     output_handle
//! #         .consolidate()
//! #         .iter()
//! #         .for_each(|((l, y, m), (vaxxes, pop), w)| {
//! #             let rate = vaxxes as f64 / pop as f64 * 100.0;
//! #             println!("{l:16} {y}-{m:02}: {vaxxes:9} {pop:8} {rate:6.2}% {w:+}",)
//! #         });
//! #     Ok(())
//! }
//! ```
//!
//! The calculation of monthly totals stays the same.  Starting from those, we
//! calculate running vaccination totals, which requires first re-indexing.
//! Then we join against the population table, which also requires a re-index
//! step:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::{
//! #         time_series::{RelOffset, RelRange},
//! #         FilterMap,
//! #     },
//! #     CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! # fn build_circuit(
//! #     circuit: &mut RootCircuit,
//! # ) -> Result<(
//! #     CollectionHandle<Record, isize>,
//! #     CollectionHandle<String, (u64, isize)>,
//! #     OutputHandle<OrdIndexedZSet<(String, i32, u8), (isize, u64), isize>>,
//! # )> {
//! #     let (vax_stream, vax_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let (pop_stream, pop_handle) = circuit.add_input_indexed_zset::<String, u64, isize>();
//! #     let subset = vax_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//!    let running_monthly_totals = monthly_totals
//!        .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//!        .partitioned_rolling_aggregate_linear(
//!            |vaxxed| *vaxxed,
//!            |total| total,
//!            RelRange::new(RelOffset::Before(u32::MAX), RelOffset::Before(0)),
//!        );
//!    let vax_rates = running_monthly_totals
//!        .map_index(|(l, (date, total))| {
//!            (
//!                l.clone(),
//!                ((date / 12) as i32, (date % 12 + 1) as u8, total.unwrap()),
//!            )
//!        })
//!        .join_index(&pop_stream, |l, (y, m, total), pop| {
//!            Some(((l.clone(), *y, *m), (*total, *pop)))
//!        });
//! #     Ok((vax_handle, pop_handle, vax_rates.output()))
//! # }
//! ```
//!
//! And finally we adjust `main` to print the results:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::{
//! #         time_series::{RelOffset, RelRange},
//! #         FilterMap,
//! #     },
//! #     CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! # fn build_circuit(
//! #     circuit: &mut RootCircuit,
//! # ) -> Result<(
//! #     CollectionHandle<Record, isize>,
//! #     CollectionHandle<String, (u64, isize)>,
//! #     OutputHandle<OrdIndexedZSet<(String, i32, u8), (isize, u64), isize>>,
//! # )> {
//! #     let (vax_stream, vax_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let (pop_stream, pop_handle) = circuit.add_input_indexed_zset::<String, u64, isize>();
//! #     let subset = vax_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let running_monthly_totals = monthly_totals
//! #         .map_index(|((l, y, m), v)| (l.clone(), (*y as u32 * 12 + (*m as u32 - 1), *v)))
//! #         .partitioned_rolling_aggregate_linear(
//! #             |vaxxed| *vaxxed,
//! #             |total| total,
//! #             RelRange::new(RelOffset::Before(u32::MAX), RelOffset::Before(0)),
//! #         );
//! #     let vax_rates = running_monthly_totals
//! #         .map_index(|(l, (date, total))| {
//! #             (
//! #                 l.clone(),
//! #                 ((date / 12) as i32, (date % 12 + 1) as u8, total.unwrap()),
//! #             )
//! #         })
//! #         .join_index(&pop_stream, |l, (y, m, total), pop| {
//! #             Some(((l.clone(), *y, *m), (*total, *pop)))
//! #         });
//! #     Ok((vax_handle, pop_handle, vax_rates.output()))
//! # }
//! #
//! # fn main() -> Result<()> {
//! #     let (circuit, (vax_handle, pop_handle, output_handle)) =
//! #         RootCircuit::build(build_circuit)?;
//! #
//! #     let path = format!(
//! #         "{}/examples/tutorial/vaccinations.csv",
//! #         env!("CARGO_MANIFEST_DIR")
//! #     );
//! #     let mut vax_records = Reader::from_path(path)?
//! #         .deserialize()
//! #         .map(|result| result.map(|record| (record, 1)))
//! #         .collect::<Result<Vec<(Record, isize)>, _>>()?;
//! #     vax_handle.append(&mut vax_records);
//! #
//! #     let mut pop_records = vec![
//! #         ("England".into(), (56286961, 1)),
//! #         ("Northern Ireland".into(), (1893667, 1)),
//! #         ("Scotland".into(), (5463300, 1)),
//! #         ("Wales".into(), (3152879, 1)),
//! #     ];
//! #     pop_handle.append(&mut pop_records);
//! #
//! #     circuit.step()?;
//! #
//!     output_handle
//!         .consolidate()
//!         .iter()
//!         .for_each(|((l, y, m), (vaxxes, pop), w)| {
//!             let rate = vaxxes as f64 / pop as f64 * 100.0;
//!             println!("{l:16} {y}-{m:02}: {vaxxes:9} {pop:8} {rate:6.2}% {w:+}",)
//!         });
//! #     Ok(())
//! # }
//! ```
//!
//! The complete program is in `tutorial8.rs`.  If we run it, it prints, in
//! part, the following. The percentages over 100% are correct because this data
//! counts vaccination doses rather than people:
//!
//! ```text
//! England          2021-01:   5600174 56286961   9.95% +1
//! England          2021-02:  14977592 56286961  26.61% +1
//! England          2021-03:  26838767 56286961  47.68% +1
//! England          2021-04:  38127712 56286961  67.74% +1
//! England          2021-05:  51900658 56286961  92.21% +1
//! England          2021-06:  62845573 56286961 111.65% +1
//! ...
//! ```
//!
//! # Incremental computation
//!
//! DBSP shines when data arrive item by item or in batches, because its
//! internals work "incrementally", that is, they do only as much
//! (re)computation as needed to reflect the input changes, rather than
//! recalculating everything from an empty state.
//!
//! Our examples so far have fed all of the input data into the circuit in one
//! go.  To demonstrate incremental computation, we can simulate data arriving
//! over time by dividing our CSV file into batches and separately pushing each
//! batch into an individual step of the circuit.  `tutorial9.rs` does that: it
//! is a copy of the program from [Finding months with the most
//! vaccinations](#finding-months-with-the-most-vaccinations) modified so that
//! it feeds data in at most 500 records per step.  The only changes from the
//! previous version are in `main`, which becomes:
//!
//! ```
//! # use anyhow::Result;
//! # use bincode::{Decode, Encode};
//! # use csv::Reader;
//! # use dbsp::{
//! #     operator::FilterMap, CollectionHandle, IndexedZSet, OrdIndexedZSet, OutputHandle, RootCircuit,
//! # };
//! # use serde::Deserialize;
//! # use size_of::SizeOf;
//! # use time::Date;
//! #
//! # #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct Record {
//! #     location: String,
//! #     #[bincode(with_serde)]
//! #     date: Date,
//! #     daily_vaccinations: Option<u64>,
//! # }
//! #
//! # #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, SizeOf, Decode, Encode)]
//! # struct VaxMonthly {
//! #     count: u64,
//! #     year: i32,
//! #     month: u8,
//! # }
//! #
//! # fn build_circuit(
//! #     circuit: &mut RootCircuit,
//! # ) -> Result<(
//! #     CollectionHandle<Record, isize>,
//! #     OutputHandle<OrdIndexedZSet<String, VaxMonthly, isize>>,
//! # )> {
//! #     let (input_stream, input_handle) = circuit.add_input_zset::<Record, isize>();
//! #     let subset = input_stream.filter(|r| {
//! #         r.location == "England"
//! #             || r.location == "Northern Ireland"
//! #             || r.location == "Scotland"
//! #             || r.location == "Wales"
//! #     });
//! #     let monthly_totals = subset
//! #         .index_with(|r| {
//! #             (
//! #                 (r.location.clone(), r.date.year(), r.date.month() as u8),
//! #                 r.daily_vaccinations.unwrap_or(0),
//! #             )
//! #         })
//! #         .aggregate_linear(|(_l, _y, _m), v| *v as isize);
//! #     let most_vax = monthly_totals
//! #         .map_index(|((l, y, m), sum)| {
//! #             (
//! #                 l.clone(),
//! #                 VaxMonthly {
//! #                     count: *sum as u64,
//! #                     year: *y,
//! #                     month: *m,
//! #                 },
//! #             )
//! #         })
//! #         .topk_desc(3);
//! #     Ok((input_handle, most_vax.output()))
//! # }
//! #
//! fn main() -> Result<()> {
//!     let (circuit, (input_handle, output_handle)) = RootCircuit::build(build_circuit)?;
//!
//!     let path = format!(
//!         "{}/examples/tutorial/vaccinations.csv",
//!         env!("CARGO_MANIFEST_DIR")
//!     );
//!     let mut reader = Reader::from_path(path)?;
//!     let mut input_records = reader.deserialize();
//!     loop {
//!         let mut batch = Vec::new();
//!         while batch.len() < 500 {
//!             let Some(record) = input_records.next() else { break };
//!             batch.push((record?, 1));
//!         }
//!         if batch.is_empty() {
//!             break;
//!         }
//!         println!("Input {} records:", batch.len());
//!         input_handle.append(&mut batch);
//!
//!         circuit.step()?;
//!
//!         output_handle
//!             .consolidate()
//!             .iter()
//!             .for_each(|(l, VaxMonthly { count, year, month }, w)| {
//!                 if w != 0 {
//!                     println!("   {l:16} {year}-{month:02} {count:10}: {w:+}")
//!                 }
//!             });
//!         println!();
//!     }
//!     Ok(())
//! }
//! ```
//!
//! The output of our new program starts out like the following.  The first
//! batch of 500 records are data for England, so the program outputs the top 3.
//! The second batch introduces some of the data for Northern Ireland, so the
//! program outputs the initial top 3.  The third batch includes a month with
//! more vaccinations than the previous record, so it "retracts" (deletes) the
//! previous 3rd-place record, indicated by the output record with `-1` weight,
//! and inserts the new record:
//!
//! ```text
//! Input 500 records:
//!    England          2021-03   11861175: +1
//!    England          2021-05   13772946: +1
//!    England          2021-12   14801300: +1
//!
//! Input 500 records:
//!    Northern Ireland 2021-03     328990: +1
//!    Northern Ireland 2021-05     394047: +1
//!    Northern Ireland 2021-04     436870: +1
//!
//! Input 500 records:
//!    Northern Ireland 2021-03     328990: -1
//!    Northern Ireland 2021-12     489059: +1
//! ```
//!
//! # Next steps
//!
//! We've shown how input, computation, and output work in DBSP.  That's all
//! the basics.  A good next step could be to look through the methods available
//! on [`Stream`] for computation.
//!
//! As a final note, we used [`RootCircuit::build`] to create our circuits.
//! That method creates circuits that run in the current thread.  DBSP also
//! provides a multithreaded runtime.  To run our circuit in 4 worker threads
//! instead of in the current thread is as simple as importing
//! [`dbsp::Runtime`](`Runtime`) and then changing
//!
//! ```ignore
//! let (circuit, (/*handles*/)) = RootCircuit::build(build_circuit)?;
//! ```
//!
//! to:
//!
//! ```ignore
//! let (mut circuit, (/*handles*/)) = Runtime::init_circuit(4, build_circuit)?;
//! ```
use crate::{
    operator::{time_series::PartitionedIndexedZSet, Aggregator, FilterMap, Max},
    CircuitHandle, CollectionHandle, IndexedZSet, OutputHandle, RootCircuit, Runtime, Stream, ZSet,
};
