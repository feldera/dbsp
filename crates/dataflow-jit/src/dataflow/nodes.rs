use crate::{
    codegen::VTable,
    dataflow::RowZSet,
    ir::{
        nodes::{StreamKind, StreamLayout, TopkOrder},
        LayoutId, NodeId,
    },
    row::Row,
};
use dbsp::operator::time_series::RelRange;
use petgraph::prelude::DiGraphMap;
use std::collections::BTreeMap;

// FIXME: Ideally each node would carry a handle to the jit runtime so that we
// could ensure we don't deallocate anything important until the dataflow is
// totally shut down

#[derive(Debug, Clone)]
pub enum DataflowNode {
    Map(Map),
    Filter(Filter),
    FilterMap(FilterMap),
    FilterMapIndex(FilterMapIndex),
    Sum(Sum),
    Neg(Neg),
    Sink(Sink),
    Source(Source),
    SourceMap(SourceMap),
    IndexWith(IndexWith),
    Delta0(Delta0),
    DelayedFeedback(DelayedFeedback),
    Min(Min),
    Max(Max),
    Distinct(Distinct),
    JoinCore(JoinCore),
    Subgraph(DataflowSubgraph),
    Export(Export),
    Noop(Noop),
    Minus(Minus),
    MonotonicJoin(MonotonicJoin),
    Differentiate(Differentiate),
    Integrate(Integrate),
    Constant(Constant),
    Fold(Fold),
    PartitionedRollingFold(PartitionedRollingFold),
    FlatMap(FlatMap),
    Antijoin(Antijoin),
    IndexByColumn(IndexByColumn),
    UnitMapToSet(UnitMapToSet),
    Topk(Topk),
}

#[derive(Debug, Clone)]
pub struct Topk {
    pub input: NodeId,
    pub k: u64,
    pub order: TopkOrder,
}

#[derive(Debug, Clone)]
pub struct UnitMapToSet {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct IndexByColumn {
    pub input: NodeId,
    pub owned_fn:
        unsafe extern "C" fn(*mut u8, *mut u8, &'static VTable, *mut u8, &'static VTable, usize),
    pub borrowed_fn:
        unsafe extern "C" fn(*const u8, *mut u8, &'static VTable, *mut u8, &'static VTable, usize),
    pub key_vtable: &'static VTable,
    pub value_vtable: &'static VTable,
}

#[derive(Debug, Clone)]
pub struct Antijoin {
    pub lhs: NodeId,
    pub rhs: NodeId,
}

#[derive(Debug, Clone)]
pub struct FlatMap {
    pub input: NodeId,
    pub flat_map: FlatMapFn,
}

#[derive(Debug, Clone)]
pub enum FlatMapFn {
    // Set input, set output
    SetSet {
        // fn(*const input_key, keys: *mut { &mut Vec<Row>, &'static VTable })
        flat_map: unsafe extern "C" fn(*const u8, *mut [*mut u8; 2]),
        key_vtable: &'static VTable,
    },

    // Set input, map output
    SetMap {
        // ```
        // fn(
        //     key: *const input_key,
        //     output_keys: *mut { &mut Vec<Row>, &'static VTable },
        //     output_values: *mut { &mut Vec<Row>, &'static VTable },
        // )
        // ```
        flat_map: unsafe extern "C" fn(*const u8, *mut [*mut u8; 2], *mut [*mut u8; 2]),
        key_vtable: &'static VTable,
        value_vtable: &'static VTable,
    },

    // Map input, set output
    MapSet {
        // fn(*const input_key, *const input_value, keys: *mut { &mut Vec<Row>, &'static VTable })
        flat_map: unsafe extern "C" fn(*const u8, *const u8, *mut [*mut u8; 2]),
        key_vtable: &'static VTable,
    },

    // Map input, map output
    MapMap {
        // ```
        // fn(
        //     key: *const input_key,
        //     value: *const input_value,
        //     output_keys: *mut { &mut Vec<Row>, &'static VTable },
        //     output_values: *mut { &mut Vec<Row>, &'static VTable },
        // )
        // ```
        flat_map: unsafe extern "C" fn(*const u8, *const u8, *mut [*mut u8; 2], *mut [*mut u8; 2]),
        key_vtable: &'static VTable,
        value_vtable: &'static VTable,
    },
}

#[derive(Debug, Clone)]
pub struct Fold {
    pub input: NodeId,
    pub init: Row,
    pub acc_vtable: &'static VTable,
    pub step_vtable: &'static VTable,
    pub output_vtable: &'static VTable,
    pub step_fn: unsafe extern "C" fn(*mut u8, *const u8, *const u8),
    pub finish_fn: unsafe extern "C" fn(*mut u8, *mut u8),
}

#[derive(Debug, Clone)]
pub struct PartitionedRollingFold {
    pub input: NodeId,
    pub range: RelRange<i64>,
    pub init: Row,
    pub acc_vtable: &'static VTable,
    pub step_vtable: &'static VTable,
    pub output_vtable: &'static VTable,
    pub step_fn: unsafe extern "C" fn(*mut u8, *const u8, *const u8),
    pub finish_fn: unsafe extern "C" fn(*mut u8, *mut u8),
}

#[derive(Debug, Clone)]
pub struct Constant {
    pub value: RowZSet,
}

#[derive(Debug, Clone)]
pub struct Differentiate {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct Integrate {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct Minus {
    pub lhs: NodeId,
    pub rhs: NodeId,
}

#[derive(Debug, Clone)]
pub struct Noop {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct Export {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct DataflowSubgraph {
    pub edges: DiGraphMap<NodeId, ()>,
    pub inputs: BTreeMap<NodeId, NodeId>,
    pub nodes: BTreeMap<NodeId, DataflowNode>,
    pub feedback_connections: BTreeMap<NodeId, NodeId>,
}

#[derive(Debug, Clone)]
pub struct JoinCore {
    pub lhs: NodeId,
    pub rhs: NodeId,
    pub join_fn: unsafe extern "C" fn(*const u8, *const u8, *const u8, *mut u8, *mut u8),
    pub key_vtable: &'static VTable,
    pub value_vtable: &'static VTable,
    pub output_kind: StreamKind,
}

#[derive(Debug, Clone)]
pub struct MonotonicJoin {
    pub lhs: NodeId,
    pub rhs: NodeId,
    pub join_fn: unsafe extern "C" fn(*const u8, *const u8, *const u8, *mut u8),
    pub key_vtable: &'static VTable,
}

#[derive(Debug, Clone)]
pub struct Min {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct Max {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct Distinct {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct DelayedFeedback {}

#[derive(Debug, Clone)]
pub struct Source {
    pub key_layout: LayoutId,
}

#[derive(Debug, Clone)]
pub struct SourceMap {
    pub key_layout: LayoutId,
    pub value_layout: LayoutId,
}

#[derive(Debug, Clone)]
pub struct Sink {
    pub input: NodeId,
    pub layout: StreamLayout,
}

#[derive(Debug, Clone)]
pub struct IndexWith {
    pub input: NodeId,
    pub index_fn: unsafe extern "C" fn(*const u8, *mut u8, *mut u8),
    pub key_vtable: &'static VTable,
    pub value_vtable: &'static VTable,
}

#[derive(Debug, Clone)]
pub struct Map {
    pub input: NodeId,
    pub map_fn: MapFn,
}

#[derive(Debug, Clone, Copy)]
pub enum MapFn {
    SetSet {
        map: unsafe extern "C" fn(*const u8, *mut u8),
        key_vtable: &'static VTable,
    },
    SetMap {
        map: unsafe extern "C" fn(*const u8, *mut u8, *mut u8),
        key_vtable: &'static VTable,
        value_vtable: &'static VTable,
    },
    MapSet {
        map: unsafe extern "C" fn(*const u8, *const u8, *mut u8),
        key_vtable: &'static VTable,
    },
    MapMap {
        map: unsafe extern "C" fn(*const u8, *const u8, *mut u8, *mut u8),
        key_vtable: &'static VTable,
        value_vtable: &'static VTable,
    },
}

// TODO: Maybe just enum the filter function?
#[derive(Debug, Clone)]
pub struct Filter {
    pub input: NodeId,
    pub filter_fn: FilterFn,
    pub layout: StreamLayout,
}

impl Filter {
    pub const fn input(&self) -> NodeId {
        self.input
    }
}

#[derive(Debug, Clone, Copy)]
pub enum FilterFn {
    Set(unsafe extern "C" fn(*const u8) -> bool),
    Map(unsafe extern "C" fn(*const u8, *const u8) -> bool),
}

#[derive(Debug, Clone)]
pub struct FilterMap {
    pub input: NodeId,
    pub filter_map: unsafe extern "C" fn(*const u8, *mut u8) -> bool,
    pub output_vtable: &'static VTable,
}

#[derive(Debug, Clone)]
pub struct FilterMapIndex {
    pub input: NodeId,
    pub filter_map: unsafe extern "C" fn(*const u8, *const u8, *mut u8) -> bool,
    pub output_vtable: &'static VTable,
}

#[derive(Debug, Clone)]
pub struct Neg {
    pub input: NodeId,
}

#[derive(Debug, Clone)]
pub struct Sum {
    pub inputs: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct Delta0 {
    pub input: NodeId,
}
