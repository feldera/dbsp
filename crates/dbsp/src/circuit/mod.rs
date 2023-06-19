//! Synchronous circuits over streams.
//!
//! A circuit consists of [operators](`operator_traits::Operator`) connected by
//! [streams](`circuit_builder::Stream`). At every clock cycle, each operator
//! consumes a single value from each of its input streams and emits a single
//! value to the output stream (except that nested circuits can execute multiple
//! operations for each outer clock tick).
//!
//! Use `RootCircuit::build` to create and populate an circuit that executes in
//! the calling thread, or `Runtime::init_circuit` to create a multi-circuit,
//! multi-worker threaded runtime.  These functions return a [`CircuitHandle`]
//! or [`DBSPHandle`], respectively, that control the circuits' execution,
//! plus, when used in the recommended way, additional input handles for
//! feeding data into the circuits and output handles for obtaining their
//! output.

mod activations;
mod dbsp_handle;

pub(crate) mod runtime;

#[macro_use]
pub mod metadata;
pub mod cache;
pub mod circuit_builder;
pub mod operator_traits;
pub mod schedule;
pub mod trace;

pub use activations::{Activations, Activator};
pub use circuit_builder::{
    ChildCircuit, Circuit, CircuitHandle, ExportId, ExportStream, FeedbackConnector, GlobalNodeId,
    NodeId, OwnershipPreference, RootCircuit, Scope, Stream, WithClock,
};
pub use dbsp_handle::{DBSPHandle, Host, IntoLayout, Layout};
pub use runtime::{Error as RuntimeError, LocalStore, LocalStoreMarker, Runtime, RuntimeHandle};

pub use schedule::Error as SchedulerError;
