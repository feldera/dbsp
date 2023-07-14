mod nodes;
mod operators;
mod tests;

use crate::{
    codegen::{Codegen, CodegenConfig, LayoutVTable, NativeLayoutCache, VTable},
    dataflow::nodes::{
        Antijoin, DataflowSubgraph, DelayedFeedback, Delta0, Differentiate, Distinct, Export,
        FilterFn, FilterMap, FilterMapIndex, FlatMap, FlatMapFn, Fold, IndexByColumn, Integrate,
        JoinCore, MapFn, Max, Min, Minus, Noop, PartitionedRollingFold, Topk, UnitMapToSet,
    },
    ir::{
        graph,
        literal::StreamCollection,
        nodes::{
            DataflowNode as _, Node, StreamKind, StreamLayout, Subgraph as SubgraphNode, TopkOrder,
        },
        Graph, GraphExt, LayoutId, NodeId,
    },
    row::{row_from_literal, Row, UninitRow},
};
use anyhow::Result as AnyResult;
use cranelift_jit::JITModule;
use cranelift_module::FuncId;
use dbsp::{
    algebra::UnimplementedSemigroup,
    operator::{FilterMap as _, Generator},
    trace::{Batch, BatchReader, Batcher, Cursor, Spine},
    Circuit, CollectionHandle, DBTimestamp, OrdIndexedZSet, OrdZSet, OutputHandle, RootCircuit,
    Stream,
};
use derive_more::{IsVariant, Unwrap};
use nodes::{
    DataflowNode, Filter, IndexWith, Map, MonotonicJoin, Neg, Sink, Source, SourceMap, Sum,
};
use petgraph::{algo, prelude::DiGraphMap};
use std::{
    collections::BTreeMap,
    iter,
    mem::{transmute, ManuallyDrop, MaybeUninit},
    ptr::{self, NonNull},
};

// TODO: Keep layout ids in dataflow nodes so we can do assertions that types
// are correct

type RowSet = OrdZSet<Row, i32>;
type RowMap = OrdIndexedZSet<Row, Row, i32>;

type Inputs = BTreeMap<NodeId, (RowInput, StreamLayout)>;
type Outputs = BTreeMap<NodeId, (RowOutput, StreamLayout)>;

#[derive(Clone, IsVariant, Unwrap)]
pub enum RowInput {
    Set(CollectionHandle<Row, i32>),
    Map(CollectionHandle<Row, (Row, i32)>),
}

impl RowInput {
    pub fn as_set_mut(&mut self) -> Option<&mut CollectionHandle<Row, i32>> {
        if let Self::Set(handle) = self {
            Some(handle)
        } else {
            None
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut CollectionHandle<Row, (Row, i32)>> {
        if let Self::Map(handle) = self {
            Some(handle)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub enum RowOutput {
    Set(OutputHandle<RowSet>),
    Map(OutputHandle<RowMap>),
}

impl RowOutput {
    pub const fn as_set(&self) -> Option<&OutputHandle<RowSet>> {
        if let Self::Set(handle) = self {
            Some(handle)
        } else {
            None
        }
    }

    pub fn as_set_mut(&mut self) -> Option<&mut OutputHandle<RowSet>> {
        if let Self::Set(handle) = self {
            Some(handle)
        } else {
            None
        }
    }

    pub const fn as_map(&self) -> Option<&OutputHandle<RowMap>> {
        if let Self::Map(handle) = self {
            Some(handle)
        } else {
            None
        }
    }

    pub fn as_map_mut(&mut self) -> Option<&mut OutputHandle<RowMap>> {
        if let Self::Map(handle) = self {
            Some(handle)
        } else {
            None
        }
    }
}

// TODO: Change the weight to a `Row`? Toggle between `i32` and `i64`?
#[derive(Clone, IsVariant, Unwrap)]
pub enum RowStream<C> {
    Set(Stream<C, RowSet>),
    Map(Stream<C, RowMap>),
}

impl<C> RowStream<C> {
    pub const fn as_set(&self) -> Option<&Stream<C, RowSet>> {
        if let Self::Set(set) = self {
            Some(set)
        } else {
            None
        }
    }

    pub const fn as_map(&self) -> Option<&Stream<C, RowMap>> {
        if let Self::Map(map) = self {
            Some(map)
        } else {
            None
        }
    }
}

#[derive(Clone, IsVariant, Unwrap)]
pub enum RowTrace<C> {
    Set(Stream<C, Spine<RowSet>>),
    Map(Stream<C, Spine<RowMap>>),
}

#[derive(Debug, Clone)]
pub enum RowZSet {
    Set(RowSet),
    Map(RowMap),
}

pub struct JitHandle {
    pub(crate) jit: JITModule,
    vtables: BTreeMap<LayoutId, *mut VTable>,
}

impl JitHandle {
    pub fn vtables(&self) -> &BTreeMap<LayoutId, *mut VTable> {
        &self.vtables
    }

    /// Free all memory associated with the JIT compiled code, including vtables
    /// and the functions themselves
    ///
    /// # Safety
    ///
    /// Cannot call this while any vtable or function pointers are still live or
    /// in use, any attempt to use them after calling this function is UB
    pub unsafe fn free_memory(mut self) {
        for &vtable in self.vtables.values() {
            drop(Box::from_raw(vtable));
        }

        self.vtables.clear();
        self.jit.free_memory();
    }
}

#[derive(Debug, Clone)]
pub struct CompiledDataflow {
    nodes: BTreeMap<NodeId, DataflowNode>,
    edges: DiGraphMap<NodeId, ()>,
}

impl CompiledDataflow {
    pub fn new<F>(
        graph: &Graph,
        config: CodegenConfig,
        with_codegen: F,
    ) -> (Self, JitHandle, NativeLayoutCache)
    where
        F: FnOnce(&mut Codegen),
    {
        let mut node_kinds = BTreeMap::new();
        let mut node_streams: BTreeMap<NodeId, Option<_>> = BTreeMap::new();

        let (mut inputs, mut input_nodes) = (Vec::with_capacity(16), Vec::with_capacity(16));
        let order = algo::toposort(graph.edges(), None).unwrap();
        for node_id in order {
            if !graph.nodes().contains_key(&node_id) {
                continue;
            }

            let node = &graph.nodes()[&node_id];

            node.inputs(&mut input_nodes);
            inputs.extend(input_nodes.iter().filter_map(|input| node_streams[input]));

            node_kinds.insert(node_id, node.output_kind(&inputs, graph.layout_cache()));
            node_streams.insert(node_id, node.output_stream(&inputs, graph.layout_cache()));

            inputs.clear();
            input_nodes.clear();

            if let Node::Subgraph(subgraph) = node {
                collect_subgraph_inputs(
                    subgraph,
                    &mut inputs,
                    &mut input_nodes,
                    &mut node_kinds,
                    &mut node_streams,
                );
            }
        }
        drop((inputs, input_nodes));

        fn collect_subgraph_inputs(
            graph: &SubgraphNode,
            inputs: &mut Vec<StreamLayout>,
            input_nodes: &mut Vec<NodeId>,
            node_kinds: &mut BTreeMap<NodeId, Option<StreamKind>>,
            node_streams: &mut BTreeMap<NodeId, Option<StreamLayout>>,
        ) {
            let order = algo::toposort(graph.edges(), None).unwrap();
            for node_id in order {
                if graph.input_nodes().contains_key(&node_id) {
                    continue;
                }

                let node = &graph.nodes()[&node_id];
                node.inputs(input_nodes);
                inputs.extend(input_nodes.iter().filter_map(|input| node_streams[input]));

                node_kinds.insert(node_id, node.output_kind(inputs, graph.layout_cache()));
                node_streams.insert(node_id, node.output_stream(inputs, graph.layout_cache()));

                inputs.clear();
                input_nodes.clear();

                if let Node::Subgraph(subgraph) = node {
                    collect_subgraph_inputs(
                        subgraph,
                        inputs,
                        input_nodes,
                        node_kinds,
                        node_streams,
                    );
                }
            }
        }

        // Run codegen over all nodes
        let mut codegen = Codegen::new(graph.layout_cache().clone(), config);
        // TODO: SmallVec
        let mut node_functions = BTreeMap::new();
        let mut vtables = BTreeMap::new();
        collect_functions(
            &mut codegen,
            &mut node_functions,
            &mut vtables,
            graph.graph(),
        );
        with_codegen(&mut codegen);

        let (jit, native_layout_cache) = codegen.finalize_definitions();
        let vtables = vtables
            .into_iter()
            .map(|(layout, vtable)| (layout, Box::into_raw(Box::new(vtable.marshalled(&jit)))))
            .collect();
        let nodes = compile_nodes(
            graph.graph(),
            &vtables,
            &jit,
            &node_streams,
            &node_functions,
            &native_layout_cache,
        );

        (
            Self {
                nodes,
                edges: graph.edges().clone(),
            },
            JitHandle { jit, vtables },
            native_layout_cache,
        )
    }

    pub fn construct(mut self, circuit: &mut RootCircuit) -> AnyResult<(Inputs, Outputs)> {
        let mut streams = BTreeMap::<NodeId, RowStream<RootCircuit>>::new();

        let mut inputs = BTreeMap::new();
        let mut outputs = BTreeMap::new();

        let order = algo::toposort(&self.edges, None).unwrap();
        for node_id in order {
            let node = match self.nodes.remove(&node_id) {
                Some(node) => node,
                None => continue,
            };

            match node {
                DataflowNode::Map(map) => self.map(node_id, map, &mut streams),
                DataflowNode::Filter(filter) => self.filter(node_id, filter, &mut streams),
                DataflowNode::IndexByColumn(index_by) => {
                    self.index_by_column(node_id, index_by, &mut streams);
                }
                DataflowNode::UnitMapToSet(map_to_set) => {
                    self.unit_map_to_set(node_id, map_to_set, &mut streams);
                }
                DataflowNode::Fold(fold) => self.fold(node_id, fold, &mut streams),

                DataflowNode::FilterMap(map) => {
                    let input = &streams[&map.input];
                    let mapped = match input {
                        RowStream::Set(input) => {
                            let (fmap_fn, vtable) = (map.filter_map, map.output_vtable);

                            let mut output = None;
                            RowStream::Set(input.flat_map(move |input| {
                                let mut out =
                                    output.take().unwrap_or_else(|| UninitRow::new(vtable));

                                if unsafe { fmap_fn(input.as_ptr(), out.as_mut_ptr()) } {
                                    Some(unsafe { out.assume_init() })
                                } else {
                                    output = Some(out);
                                    None
                                }
                            }))
                        }

                        RowStream::Map(_) => todo!(),
                    };

                    streams.insert(node_id, mapped);
                }

                DataflowNode::FilterMapIndex(map) => {
                    let input = &streams[&map.input];
                    let mapped = match input {
                        RowStream::Map(input) => {
                            let (fmap_fn, vtable) = (map.filter_map, map.output_vtable);

                            let mut output = None;
                            RowStream::Set(input.flat_map(move |(key, value)| {
                                let mut out =
                                    output.take().unwrap_or_else(|| UninitRow::new(vtable));

                                let keep_element = unsafe {
                                    fmap_fn(key.as_ptr(), value.as_ptr(), out.as_mut_ptr())
                                };
                                if keep_element {
                                    Some(unsafe { out.assume_init() })
                                } else {
                                    output = Some(out);
                                    None
                                }
                            }))
                        }

                        RowStream::Set(_) => todo!(),
                    };

                    streams.insert(node_id, mapped);
                }

                DataflowNode::FlatMap(flat_map) => self.flat_map(node_id, flat_map, &mut streams),

                DataflowNode::IndexWith(index_with) => {
                    let input = &streams[&index_with.input];
                    let (index_fn, key_vtable, value_vtable) = (
                        index_with.index_fn,
                        index_with.key_vtable,
                        index_with.value_vtable,
                    );

                    let indexed = match input {
                        RowStream::Set(input) => input.index_with(move |input| {
                            let (mut key_output, mut value_output) =
                                (UninitRow::new(key_vtable), UninitRow::new(value_vtable));

                            unsafe {
                                index_fn(
                                    input.as_ptr(),
                                    key_output.as_mut_ptr(),
                                    value_output.as_mut_ptr(),
                                );

                                (key_output.assume_init(), value_output.assume_init())
                            }
                        }),

                        // FIXME: `.index_with()` requires that `Key` is a `()`
                        RowStream::Map(_) => todo!(),
                    };

                    streams.insert(node_id, RowStream::Map(indexed));
                }

                DataflowNode::Sum(sum) => {
                    let sum = match &streams[&sum.inputs[0]] {
                        RowStream::Set(first) => {
                            RowStream::Set(first.sum(sum.inputs[1..].iter().map(|input| {
                                if let RowStream::Set(input) = &streams[input] {
                                    input
                                } else {
                                    unreachable!()
                                }
                            })))
                        }
                        RowStream::Map(first) => {
                            RowStream::Map(first.sum(sum.inputs[1..].iter().map(|input| {
                                if let RowStream::Map(input) = &streams[input] {
                                    input
                                } else {
                                    unreachable!()
                                }
                            })))
                        }
                    };
                    streams.insert(node_id, sum);
                }

                DataflowNode::Minus(minus) => {
                    let lhs = &streams[&minus.lhs];
                    let rhs = streams[&minus.rhs].clone();
                    let difference = match lhs {
                        RowStream::Set(first) => RowStream::Set(first.minus(&rhs.unwrap_set())),
                        RowStream::Map(first) => RowStream::Map(first.minus(&rhs.unwrap_map())),
                    };
                    streams.insert(node_id, difference);
                }

                DataflowNode::Neg(neg) => self.neg(node_id, neg, &mut streams),

                DataflowNode::Differentiate(diff) => {
                    let input = &streams[&diff.input];
                    let differentiated = match input {
                        RowStream::Set(input) => RowStream::Set(input.differentiate()),
                        RowStream::Map(input) => RowStream::Map(input.differentiate()),
                    };

                    streams.insert(node_id, differentiated);
                }

                DataflowNode::Integrate(integrate) => {
                    let input = &streams[&integrate.input];
                    // TODO: It'd be better if we used `.integrate_trace()`
                    let integrated = match input {
                        RowStream::Set(input) => RowStream::Set(input.integrate()),
                        RowStream::Map(input) => RowStream::Map(input.integrate()),
                    };

                    streams.insert(node_id, integrated);
                }

                DataflowNode::Sink(sink) => {
                    let input = &streams[&sink.input];
                    let output = match input {
                        RowStream::Set(input) => RowOutput::Set(input.output()),
                        RowStream::Map(input) => RowOutput::Map(input.output()),
                    };

                    outputs.insert(node_id, (output, sink.layout));
                }

                DataflowNode::Source(source) => {
                    let (stream, handle) = circuit.add_input_zset::<Row, i32>();

                    if cfg!(debug_assertions) {
                        let key_layout = source.key_layout;
                        stream.inspect(move |row| {
                            tracing::trace!("running assertions over source {node_id}");

                            let mut cursor = row.cursor();
                            while cursor.key_valid() {
                                let key = cursor.key();
                                assert_eq!(key.vtable().layout_id, key_layout);
                                cursor.step_key();
                            }
                        });
                    }

                    streams.insert(node_id, RowStream::Set(stream));
                    inputs.insert(
                        node_id,
                        (RowInput::Set(handle), StreamLayout::Set(source.key_layout)),
                    );
                }

                DataflowNode::SourceMap(source) => {
                    let (stream, handle) = circuit.add_input_indexed_zset::<Row, Row, i32>();

                    if cfg!(debug_assertions) {
                        let (key_layout, value_layout) = (source.key_layout, source.value_layout);
                        stream.inspect(move |row| {
                            tracing::trace!("running assertions over source {node_id}");

                            let mut cursor = row.cursor();
                            while cursor.key_valid() {
                                while cursor.val_valid() {
                                    let key = cursor.key();
                                    assert_eq!(key.vtable().layout_id, key_layout);

                                    let value = cursor.val();
                                    assert_eq!(value.vtable().layout_id, value_layout);

                                    cursor.step_val();
                                }

                                cursor.step_key();
                            }
                        });
                    }

                    streams.insert(node_id, RowStream::Map(stream));
                    inputs.insert(
                        node_id,
                        (
                            RowInput::Map(handle),
                            StreamLayout::Map(source.key_layout, source.value_layout),
                        ),
                    );
                }

                DataflowNode::Delta0(_) => todo!(),

                DataflowNode::DelayedFeedback(_) => todo!(),

                DataflowNode::Min(min) => {
                    let min = match &streams[&min.input] {
                        RowStream::Set(_) => todo!(),
                        RowStream::Map(input) => {
                            RowStream::Map(input.aggregate_generic(dbsp::operator::Min))
                        }
                    };
                    streams.insert(node_id, min);
                }

                DataflowNode::Max(max) => {
                    let max = match &streams[&max.input] {
                        RowStream::Set(_) => todo!(),
                        RowStream::Map(input) => {
                            RowStream::Map(input.aggregate_generic(dbsp::operator::Max))
                        }
                    };
                    streams.insert(node_id, max);
                }

                DataflowNode::PartitionedRollingFold(_fold) => {
                    /*
                    let (step_fn, finish_fn) = (fold.step_fn, fold.finish_fn);
                    let (acc_vtable, step_vtable, output_vtable) =
                        (fold.acc_vtable, fold.step_vtable, fold.output_vtable);

                    let folded = match &streams[&fold.input] {
                        RowStream::Set(_) => todo!(),

                        RowStream::Map(input) => {
                            let fold_agg = dbsp::operator::Fold::<
                                _,
                                UnimplementedSemigroup<_>,
                                _,
                                _,
                            >::with_output(
                                fold.init,
                                move |acc: &mut Row, step: &Row, weight| unsafe {
                                    debug_assert_eq!(acc.vtable().layout_id, acc_vtable.layout_id);
                                    debug_assert_eq!(
                                        step.vtable().layout_id,
                                        step_vtable.layout_id
                                    );

                                    step_fn(
                                        acc.as_mut_ptr(),
                                        step.as_ptr(),
                                        &weight as *const i32 as *const u8,
                                    );
                                },
                                move |mut acc: Row| unsafe {
                                    debug_assert_eq!(acc.vtable().layout_id, acc_vtable.layout_id);

                                    let mut row = UninitRow::new(output_vtable);
                                    finish_fn(acc.as_mut_ptr(), row.as_mut_ptr());
                                    row.assume_init()
                                },
                            );

                            input.partitioned_rolling_aggregate(fold_agg, fold.range)
                        }
                    };
                    streams.insert(node_id, RowStream::Map(folded));
                    */
                    unimplemented!()
                }

                DataflowNode::Distinct(distinct) => self.distinct(node_id, distinct, &mut streams),

                DataflowNode::JoinCore(join) => {
                    let lhs = streams[&join.lhs].clone();
                    let rhs = streams[&join.rhs].clone();
                    let (join_fn, key_vtable, _value_vtable) =
                        (join.join_fn, join.key_vtable, join.value_vtable);

                    let joined = match join.output_kind {
                        StreamKind::Set => RowStream::Set(lhs.unwrap_map().join_generic(
                            &rhs.unwrap_map(),
                            move |key, lhs_val, rhs_val| {
                                let mut output = UninitRow::new(key_vtable);
                                unsafe {
                                    join_fn(
                                        key.as_ptr(),
                                        lhs_val.as_ptr(),
                                        rhs_val.as_ptr(),
                                        output.as_mut_ptr(),
                                        NonNull::<u8>::dangling().as_ptr(),
                                    );
                                }

                                iter::once((unsafe { output.assume_init() }, ()))
                            },
                        )),

                        StreamKind::Map => todo!(),
                    };
                    streams.insert(node_id, joined);
                }

                DataflowNode::MonotonicJoin(join) => {
                    let lhs = &streams[&join.lhs];
                    let rhs = streams[&join.rhs].clone();
                    let (join_fn, key_vtable) = (join.join_fn, join.key_vtable);

                    let joined = match lhs {
                        RowStream::Set(lhs) => {
                            RowStream::Set(lhs.monotonic_stream_join::<_, _, _>(
                                &rhs.unwrap_set(),
                                move |key, lhs_val, rhs_val| {
                                    let mut output = UninitRow::new(key_vtable);
                                    unsafe {
                                        join_fn(
                                            key.as_ptr(),
                                            lhs_val as *const () as *const u8,
                                            rhs_val as *const () as *const u8,
                                            output.as_mut_ptr(),
                                        );

                                        output.assume_init()
                                    }
                                },
                            ))
                        }

                        RowStream::Map(lhs) => {
                            RowStream::Set(lhs.monotonic_stream_join::<_, _, _>(
                                &rhs.unwrap_map(),
                                move |key, lhs_val, rhs_val| {
                                    let mut output = UninitRow::new(key_vtable);
                                    unsafe {
                                        join_fn(
                                            key.as_ptr(),
                                            lhs_val.as_ptr(),
                                            rhs_val.as_ptr(),
                                            output.as_mut_ptr(),
                                        );

                                        output.assume_init()
                                    }
                                },
                            ))
                        }
                    };
                    streams.insert(node_id, joined);
                }

                DataflowNode::Antijoin(antijoin) => self.antijoin(node_id, antijoin, &mut streams),

                DataflowNode::Topk(topk) => self.topk(node_id, topk, &mut streams),

                DataflowNode::Export(_) => todo!(),

                DataflowNode::Constant(constant) => {
                    self.constant(node_id, constant, circuit, &mut streams);
                }

                DataflowNode::Subgraph(subgraph) => self.subgraph(subgraph, circuit, &mut streams),

                DataflowNode::Noop(_) => {}
            }
        }

        Ok((inputs, outputs))
    }

    fn subgraph(
        &mut self,
        mut subgraph: DataflowSubgraph,
        circuit: &mut RootCircuit,
        streams: &mut BTreeMap<NodeId, RowStream<RootCircuit>>,
    ) {
        let mut needs_consolidate = BTreeMap::new();

        circuit
            .fixedpoint(|subcircuit| {
                let mut substreams = BTreeMap::new();
                let mut feedbacks = BTreeMap::new();

                let nodes = algo::toposort(&subgraph.edges, None).unwrap();
                for node_id in nodes {
                    if subgraph.inputs.contains_key(&node_id) {
                        continue;
                    }

                    let node = match subgraph.nodes.remove(&node_id) {
                        Some(node) => node,
                        None => continue,
                    };
                    match node {
                        DataflowNode::Constant(constant) => {
                            self.constant(node_id, constant, subcircuit, &mut substreams);
                        }
                        DataflowNode::Map(map) => self.map(node_id, map, &mut substreams),
                        DataflowNode::Filter(filter) => {
                            self.filter(node_id, filter, &mut substreams);
                        }
                        DataflowNode::IndexByColumn(index_by) => {
                            self.index_by_column(node_id, index_by, &mut substreams);
                        }
                        DataflowNode::UnitMapToSet(map_to_set) => {
                            self.unit_map_to_set(node_id, map_to_set, &mut substreams);
                        }
                        DataflowNode::Fold(fold) => self.fold(node_id, fold, &mut substreams),

                        DataflowNode::FilterMap(map) => {
                            let input = &substreams[&map.input];
                            let mapped = match input {
                                RowStream::Set(input) => {
                                    let (fmap_fn, vtable) = (map.filter_map, map.output_vtable);

                                    let mut output = None;
                                    RowStream::Set(input.flat_map(move |input| {
                                        let mut out =
                                            output.take().unwrap_or_else(|| UninitRow::new(vtable));
                                        unsafe {
                                            if fmap_fn(input.as_ptr(), out.as_mut_ptr()) {
                                                Some(out.assume_init())
                                            } else {
                                                output = Some(out);
                                                None
                                            }
                                        }
                                    }))
                                }

                                RowStream::Map(_) => todo!(),
                            };

                            substreams.insert(node_id, mapped);
                        }

                        DataflowNode::FilterMapIndex(map) => {
                            let input = &substreams[&map.input];
                            let mapped = match input {
                                RowStream::Map(input) => {
                                    let (fmap_fn, vtable) = (map.filter_map, map.output_vtable);

                                    let mut output = None;
                                    RowStream::Set(input.flat_map(move |(key, value)| {
                                        let mut out =
                                            output.take().unwrap_or_else(|| UninitRow::new(vtable));

                                        unsafe {
                                            if fmap_fn(
                                                key.as_ptr(),
                                                value.as_ptr(),
                                                out.as_mut_ptr(),
                                            ) {
                                                Some(out.assume_init())
                                            } else {
                                                output = Some(out);
                                                None
                                            }
                                        }
                                    }))
                                }

                                RowStream::Set(_) => todo!(),
                            };

                            substreams.insert(node_id, mapped);
                        }

                        DataflowNode::FlatMap(flat_map) => {
                            self.flat_map(node_id, flat_map, &mut substreams);
                        }

                        DataflowNode::IndexWith(index_with) => {
                            let input = &substreams[&index_with.input];
                            let (index_fn, key_vtable, value_vtable) = (
                                index_with.index_fn,
                                index_with.key_vtable,
                                index_with.value_vtable,
                            );

                            let indexed = match input {
                                RowStream::Set(input) => input.index_with(move |input| {
                                    tracing::trace!(
                                        "running IndexWith {node_id} with input {input:?}",
                                    );

                                    let (mut key_output, mut value_output) =
                                        (UninitRow::new(key_vtable), UninitRow::new(value_vtable));

                                    unsafe {
                                        index_fn(
                                            input.as_ptr(),
                                            key_output.as_mut_ptr(),
                                            value_output.as_mut_ptr(),
                                        );

                                        (key_output.assume_init(), value_output.assume_init())
                                    }
                                }),

                                // FIXME: `.index_with()` requires that `Key` is a `()`
                                RowStream::Map(_) => todo!(),
                            };

                            substreams.insert(node_id, RowStream::Map(indexed));
                        }

                        DataflowNode::Sum(sum) => {
                            let sum = match &substreams[&sum.inputs[0]] {
                                RowStream::Set(first) => {
                                    RowStream::Set(first.sum(sum.inputs[1..].iter().map(|input| {
                                        if let RowStream::Set(input) = &substreams[input] {
                                            input
                                        } else {
                                            unreachable!()
                                        }
                                    })))
                                }
                                RowStream::Map(first) => {
                                    RowStream::Map(first.sum(sum.inputs[1..].iter().map(|input| {
                                        if let RowStream::Map(input) = &substreams[input] {
                                            input
                                        } else {
                                            unreachable!()
                                        }
                                    })))
                                }
                            };
                            substreams.insert(node_id, sum);
                        }

                        DataflowNode::Minus(minus) => {
                            let lhs = &substreams[&minus.lhs];
                            let rhs = substreams[&minus.rhs].clone();
                            let difference = match lhs {
                                RowStream::Set(first) => {
                                    RowStream::Set(first.minus(&rhs.unwrap_set()))
                                }
                                RowStream::Map(first) => {
                                    RowStream::Map(first.minus(&rhs.unwrap_map()))
                                }
                            };
                            substreams.insert(node_id, difference);
                        }

                        DataflowNode::Neg(neg) => self.neg(node_id, neg, &mut substreams),

                        DataflowNode::Sink(_)
                        | DataflowNode::Source(_)
                        | DataflowNode::SourceMap(_) => todo!(),

                        DataflowNode::Delta0(delta) => {
                            let input = &streams[&delta.input];
                            let delta0 = match input {
                                RowStream::Set(input) => RowStream::Set(input.delta0(subcircuit)),
                                RowStream::Map(input) => RowStream::Map(input.delta0(subcircuit)),
                            };

                            substreams.insert(node_id, delta0);
                        }

                        DataflowNode::DelayedFeedback(_feedback) => {
                            let feedback =
                                dbsp::operator::DelayedFeedback::<_, RowSet>::new(subcircuit);
                            let stream = feedback.stream().clone();
                            substreams.insert(node_id, RowStream::Set(stream));
                            feedbacks.insert(node_id, feedback);
                        }

                        DataflowNode::Min(min) => {
                            let min = match &substreams[&min.input] {
                                RowStream::Set(_) => todo!(),
                                RowStream::Map(input) => {
                                    RowStream::Map(input.aggregate_generic(dbsp::operator::Min))
                                }
                            };
                            substreams.insert(node_id, min);
                        }

                        DataflowNode::Max(max) => {
                            let max = match &substreams[&max.input] {
                                RowStream::Set(_) => todo!(),
                                RowStream::Map(input) => {
                                    RowStream::Map(input.aggregate_generic(dbsp::operator::Max))
                                }
                            };
                            substreams.insert(node_id, max);
                        }

                        DataflowNode::PartitionedRollingFold(_fold) => {
                            /*
                            let (step_fn, finish_fn) = (fold.step_fn, fold.finish_fn);
                            let (acc_vtable, step_vtable, output_vtable) =
                                (fold.acc_vtable, fold.step_vtable, fold.output_vtable);

                            let folded = match &streams[&fold.input] {
                                RowStream::Set(_) => todo!(),

                                RowStream::Map(input) => {
                                    let fold_agg = dbsp::operator::Fold::<
                                        _,
                                        UnimplementedSemigroup<_>,
                                        _,
                                        _,
                                    >::with_output(
                                        fold.init,
                                        move |acc: &mut Row, step: &Row, weight| unsafe {
                                            debug_assert_eq!(acc.vtable().layout_id, acc_vtable.layout_id);
                                            debug_assert_eq!(
                                                step.vtable().layout_id,
                                                step_vtable.layout_id
                                            );

                                            step_fn(
                                                acc.as_mut_ptr(),
                                                step.as_ptr(),
                                                &weight as *const i32 as *const u8,
                                            );
                                        },
                                        move |mut acc: Row| unsafe {
                                            debug_assert_eq!(acc.vtable().layout_id, acc_vtable.layout_id);

                                            let mut row = UninitRow::new(output_vtable);
                                            finish_fn(acc.as_mut_ptr(), row.as_mut_ptr());
                                            (0, row.assume_init())
                                        },
                                    );

                                    input.partitioned_rolling_aggregate::<i32, Row, _>(fold_agg, fold.range)
                                }
                            };
                            streams.insert(node_id, RowStream::Map(folded));
                             */
                            unimplemented!()
                        }

                        DataflowNode::Distinct(distinct) => {
                            self.distinct(node_id, distinct, &mut substreams);
                        }

                        DataflowNode::JoinCore(join) => {
                            let lhs = substreams[&join.lhs].clone();
                            let rhs = substreams[&join.rhs].clone();
                            let (join_fn, key_vtable, _value_vtable) =
                                (join.join_fn, join.key_vtable, join.value_vtable);

                            let joined = match join.output_kind {
                                StreamKind::Set => RowStream::Set(lhs.unwrap_map().join_generic(
                                    &rhs.unwrap_map(),
                                    move |key, lhs_val, rhs_val| {
                                        tracing::trace!("running JoinCore {node_id} with input {key:?}, {lhs_val:?}, {rhs_val:?}");

                                        let mut output = UninitRow::new(key_vtable);
                                        unsafe {
                                            join_fn(
                                                key.as_ptr(),
                                                lhs_val.as_ptr(),
                                                rhs_val.as_ptr(),
                                                output.as_mut_ptr(),
                                                NonNull::<u8>::dangling().as_ptr(),
                                            );
                                        }

                                        iter::once((unsafe { output.assume_init() }, ()))
                                    },
                                )),

                                StreamKind::Map => todo!(),
                            };
                            substreams.insert(node_id, joined);
                        }

                        DataflowNode::MonotonicJoin(join) => {
                            let lhs = &substreams[&join.lhs];
                            let rhs = substreams[&join.rhs].clone();
                            let (join_fn, key_vtable) = (join.join_fn, join.key_vtable);

                            let joined = match lhs {
                                RowStream::Set(lhs) => {
                                    RowStream::Set(lhs.monotonic_stream_join::<_, _, _>(
                                        &rhs.unwrap_set(),
                                        move |key, lhs_val, rhs_val| {
                                            let mut output = UninitRow::new(key_vtable);
                                            unsafe {
                                                join_fn(
                                                    key.as_ptr(),
                                                    lhs_val as *const () as *const u8,
                                                    rhs_val as *const () as *const u8,
                                                    output.as_mut_ptr(),
                                                );

                                                output.assume_init()
                                            }
                                        },
                                    ))
                                }

                                RowStream::Map(lhs) => {
                                    RowStream::Set(lhs.monotonic_stream_join::<_, _, _>(
                                        &rhs.unwrap_map(),
                                        move |key, lhs_val, rhs_val| {
                                            let mut output = UninitRow::new(key_vtable);
                                            unsafe {
                                                join_fn(
                                                    key.as_ptr(),
                                                    lhs_val.as_ptr(),
                                                    rhs_val.as_ptr(),
                                                    output.as_mut_ptr(),
                                                );

                                                output.assume_init()
                                            }
                                        },
                                    ))
                                }
                            };
                            substreams.insert(node_id, joined);
                        }

                        DataflowNode::Antijoin(antijoin) => {
                            self.antijoin(node_id, antijoin, &mut substreams);
                        }

                        DataflowNode::Topk(_topk) => {
                            unreachable!("topk only works in top level circuits");
                        }

                        DataflowNode::Export(export) => {
                            let exported = match &substreams[&export.input] {
                                RowStream::Set(input) => {
                                    RowTrace::Set(input.integrate_trace().export())
                                }
                                RowStream::Map(input) => {
                                    RowTrace::Map(input.integrate_trace().export())
                                }
                            };

                            needs_consolidate.insert(node_id, exported);
                        }

                        DataflowNode::Differentiate(diff) => {
                            let input = &streams[&diff.input];
                            let differentiated = match input {
                                RowStream::Set(input) => {
                                    RowStream::Set(input.differentiate_nested())
                                }
                                RowStream::Map(input) => {
                                    RowStream::Map(input.differentiate_nested())
                                }
                            };

                            streams.insert(node_id, differentiated);
                        }

                        DataflowNode::Integrate(diff) => {
                            let input = &streams[&diff.input];
                            let integrated = match input {
                                RowStream::Set(input) => RowStream::Set(input.integrate_nested()),
                                RowStream::Map(input) => RowStream::Map(input.integrate_nested()),
                            };

                            streams.insert(node_id, integrated);
                        }

                        DataflowNode::Subgraph(_) => todo!(),

                        DataflowNode::Noop(_) => {}
                    }
                }

                // Connect all feedback nodes
                for (source, feedback) in subgraph.feedback_connections.iter() {
                    let source = substreams[source].clone().unwrap_set();
                    let feedback = feedbacks.remove(feedback).unwrap();
                    feedback.connect(&source);
                }

                Ok(())
            })
            .unwrap();

        for (node_id, stream) in needs_consolidate {
            let consolidated = match stream {
                RowTrace::Set(stream) => RowStream::Set(stream.consolidate()),
                RowTrace::Map(stream) => RowStream::Map(stream.consolidate()),
            };
            streams.insert(node_id, consolidated);
        }
    }

    fn distinct<C>(
        &mut self,
        node_id: NodeId,
        distinct: Distinct,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
        C::Time: DBTimestamp,
    {
        let distinct = match &streams[&distinct.input] {
            RowStream::Set(input) => RowStream::Set(input.distinct()),
            RowStream::Map(input) => RowStream::Map(input.distinct()),
        };
        streams.insert(node_id, distinct);
    }

    fn flat_map<C>(
        &mut self,
        node_id: NodeId,
        flat_map: FlatMap,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
    {
        let input = &streams[&flat_map.input];
        let mapped = match flat_map.flat_map {
            FlatMapFn::SetSet {
                flat_map,
                key_vtable,
            } => {
                let input = input.as_set().unwrap();
                let flat_map =
                    operators::FlatMap::new(move |key: &Row, output_keys: &mut Vec<Row>| unsafe {
                        flat_map(
                            key.as_ptr(),
                            &mut [
                                output_keys as *mut _ as *mut u8,
                                key_vtable as *const _ as *mut u8,
                            ],
                        )
                    });
                RowStream::Set(input.circuit().add_unary_operator(flat_map, input))
            }

            FlatMapFn::SetMap {
                flat_map,
                key_vtable,
                value_vtable,
            } => {
                let input = input.as_set().unwrap();
                let flat_map = operators::FlatMap::new(
                    move |key: &Row, output_keys: &mut Vec<Row>, output_values: &mut Vec<Row>| unsafe {
                        flat_map(
                            key.as_ptr(),
                            &mut [
                                output_keys as *mut _ as *mut u8,
                                key_vtable as *const _ as *mut u8,
                            ],
                            &mut [
                                output_values as *mut _ as *mut u8,
                                value_vtable as *const _ as *mut u8,
                            ],
                        )
                    },
                );
                RowStream::Map(input.circuit().add_unary_operator(flat_map, input))
            }

            FlatMapFn::MapSet {
                flat_map,
                key_vtable,
            } => {
                let input = input.as_map().unwrap();
                let flat_map = operators::FlatMap::new(
                    move |key: &Row, value: &Row, output_keys: &mut Vec<Row>| unsafe {
                        flat_map(
                            key.as_ptr(),
                            value.as_ptr(),
                            &mut [
                                output_keys as *mut _ as *mut u8,
                                key_vtable as *const _ as *mut u8,
                            ],
                        )
                    },
                );
                RowStream::Set(input.circuit().add_unary_operator(flat_map, input))
            }

            FlatMapFn::MapMap {
                flat_map,
                key_vtable,
                value_vtable,
            } => {
                let input = input.as_map().unwrap();
                let flat_map = operators::FlatMap::new(
                    move |key: &Row,
                          value: &Row,
                          output_keys: &mut Vec<Row>,
                          output_values: &mut Vec<Row>| unsafe {
                        flat_map(
                            key.as_ptr(),
                            value.as_ptr(),
                            &mut [
                                output_keys as *mut _ as *mut u8,
                                key_vtable as *const _ as *mut u8,
                            ],
                            &mut [
                                output_values as *mut _ as *mut u8,
                                value_vtable as *const _ as *mut u8,
                            ],
                        )
                    },
                );
                RowStream::Map(input.circuit().add_unary_operator(flat_map, input))
            }
        };

        streams.insert(node_id, mapped);
    }

    fn antijoin<C>(
        &self,
        node_id: NodeId,
        antijoin: Antijoin,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
        C::Time: DBTimestamp,
    {
        let antijoined = match (&streams[&antijoin.lhs], &streams[&antijoin.rhs]) {
            (RowStream::Set(lhs), RowStream::Set(rhs)) => RowStream::Set(lhs.antijoin(rhs)),
            (RowStream::Set(lhs), RowStream::Map(rhs)) => RowStream::Set(lhs.antijoin(rhs)),
            (RowStream::Map(lhs), RowStream::Set(rhs)) => RowStream::Map(lhs.antijoin(rhs)),
            (RowStream::Map(lhs), RowStream::Map(rhs)) => RowStream::Map(lhs.antijoin(rhs)),
        };

        streams.insert(node_id, antijoined);
    }

    fn neg<C>(&self, node_id: NodeId, neg: Neg, streams: &mut BTreeMap<NodeId, RowStream<C>>)
    where
        C: Circuit,
    {
        let negated = match &streams[&neg.input] {
            RowStream::Set(input) => RowStream::Set(input.neg()),
            RowStream::Map(input) => RowStream::Map(input.neg()),
        };

        streams.insert(node_id, negated);
    }

    fn filter<C>(
        &self,
        node_id: NodeId,
        filter: Filter,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
    {
        let filtered = match (filter.filter_fn, &streams[&filter.input()]) {
            (FilterFn::Set(filter_fn), RowStream::Set(input)) => {
                let filtered = input.filter(move |input| unsafe { filter_fn(input.as_ptr()) });
                RowStream::Set(filtered)
            }

            (FilterFn::Map(filter_fn), RowStream::Map(input)) => {
                let filtered = input
                    .filter(move |(key, value)| unsafe { filter_fn(key.as_ptr(), value.as_ptr()) });
                RowStream::Map(filtered)
            }

            _ => unreachable!(),
        };

        streams.insert(node_id, filtered);
    }

    fn constant<C>(
        &self,
        node_id: NodeId,
        constant: nodes::Constant,
        circuit: &mut C,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
    {
        let constant = match constant.value {
            RowZSet::Set(set) => {
                RowStream::Set(circuit.add_source(Generator::new(move || set.clone())))
            }

            RowZSet::Map(map) => {
                RowStream::Map(circuit.add_source(Generator::new(move || map.clone())))
            }
        };
        streams.insert(node_id, constant);
    }

    fn map<C>(&self, node_id: NodeId, map: Map, streams: &mut BTreeMap<NodeId, RowStream<C>>)
    where
        C: Circuit,
    {
        let input = streams[&map.input].clone();

        let mapped = match map.map_fn {
            MapFn::SetSet { map, key_vtable } => {
                RowStream::Set(input.unwrap_set().map(move |input| {
                    let mut output = UninitRow::new(key_vtable);
                    unsafe {
                        map(input.as_ptr(), output.as_mut_ptr());
                        output.assume_init()
                    }
                }))
            }

            MapFn::SetMap {
                map,
                key_vtable,
                value_vtable,
            } => RowStream::Map(input.unwrap_set().map_index(move |input| {
                let (mut key_output, mut value_output) =
                    (UninitRow::new(key_vtable), UninitRow::new(value_vtable));
                unsafe {
                    map(
                        input.as_ptr(),
                        key_output.as_mut_ptr(),
                        value_output.as_mut_ptr(),
                    );
                    (key_output.assume_init(), value_output.assume_init())
                }
            })),

            MapFn::MapSet { map, key_vtable } => {
                RowStream::Set(input.unwrap_map().map(move |(key, value)| {
                    let mut key_output = UninitRow::new(key_vtable);
                    unsafe {
                        map(key.as_ptr(), value.as_ptr(), key_output.as_mut_ptr());
                        key_output.assume_init()
                    }
                }))
            }

            MapFn::MapMap {
                map,
                key_vtable,
                value_vtable,
            } => RowStream::Map(input.unwrap_map().map_index(move |(key, value)| {
                let (mut key_output, mut value_output) =
                    (UninitRow::new(key_vtable), UninitRow::new(value_vtable));
                unsafe {
                    map(
                        key.as_ptr(),
                        value.as_ptr(),
                        key_output.as_mut_ptr(),
                        value_output.as_mut_ptr(),
                    );
                    (key_output.assume_init(), value_output.assume_init())
                }
            })),
        };

        streams.insert(node_id, mapped);
    }

    fn index_by_column<C>(
        &self,
        node_id: NodeId,
        index_by: IndexByColumn,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
    {
        let IndexByColumn {
            input,
            owned_fn,
            borrowed_fn,
            key_vtable,
            value_vtable,
        } = index_by;
        let input = streams[&input].as_set().unwrap();

        let indexed = input.apply_core(
            "IndexByColumn",
            move |owned| {
                let (inputs, mut diffs, lower_bound) = owned.layer.into_parts();
                // Remove all diffs at `..lower_bound`
                diffs.drain(..lower_bound).for_each(|_| ());

                // Make `inputs` contain `MaybeUninit<Row>`s so we can drop the keys at
                // `..lower_bound` without shifting the vec
                let mut inputs = cast_uninit_vec(inputs);
                // Drop all keys below `lower_bound`
                unsafe { ptr::drop_in_place(&mut inputs[..lower_bound] as *mut [_] as *mut [Row]) }

                debug_assert_eq!(inputs[lower_bound..].len(), diffs.len());

                let length = inputs.len() - lower_bound;

                let mut key_output: Vec<Row> = Vec::with_capacity(length);
                let mut value_output: Vec<Row> = Vec::with_capacity(length);

                // TODO: We could pre-sort the inputs so that we could directly create
                // an `OrdIndexedZSet` after splitting into keys and values
                unsafe {
                    owned_fn(
                        inputs.as_mut_ptr().add(lower_bound).cast(),
                        key_output.as_mut_ptr().cast(),
                        key_vtable,
                        value_output.as_mut_ptr().cast(),
                        value_vtable,
                        length,
                    );

                    key_output.set_len(length);
                    value_output.set_len(length);
                }

                // TODO: We'd rather construct the index directly
                assert!(key_output.len() == value_output.len() && key_output.len() == diffs.len());
                let mut batch = key_output
                    .into_iter()
                    .zip(value_output)
                    .zip(diffs)
                    .collect();

                let mut batcher =
                    <OrdIndexedZSet<Row, Row, i32> as Batch>::Batcher::new_batcher(());
                batcher.push_batch(&mut batch);
                batcher.seal()
            },
            move |borrowed| {
                let (inputs, diffs, lower_bound) = borrowed.layer.as_parts();
                let (inputs, diffs) = (&inputs[lower_bound..], &diffs[lower_bound..]);

                let mut key_output: Vec<Row> = Vec::with_capacity(inputs.len());
                let mut value_output: Vec<Row> = Vec::with_capacity(inputs.len());

                unsafe {
                    borrowed_fn(
                        inputs.as_ptr().cast(),
                        key_output.as_mut_ptr().cast(),
                        key_vtable,
                        value_output.as_mut_ptr().cast(),
                        value_vtable,
                        inputs.len(),
                    );

                    key_output.set_len(inputs.len());
                    value_output.set_len(inputs.len());
                }

                // TODO: We'd rather construct the index directly
                assert!(key_output.len() == value_output.len() && key_output.len() == diffs.len());
                let mut batch = key_output
                    .into_iter()
                    .zip(value_output)
                    .zip(diffs.iter().copied())
                    .collect();

                let mut batcher =
                    <OrdIndexedZSet<Row, Row, i32> as Batch>::Batcher::new_batcher(());
                batcher.push_batch(&mut batch);
                batcher.seal()
            },
            |_scope| true,
        );

        streams.insert(node_id, RowStream::Map(indexed));
    }

    fn unit_map_to_set<C>(
        &self,
        node_id: NodeId,
        map_to_set: UnitMapToSet,
        streams: &mut BTreeMap<NodeId, RowStream<C>>,
    ) where
        C: Circuit,
    {
        let input = streams[&map_to_set.input].as_map().unwrap();
        let set = input.apply_core(
            "UnitMapToSet",
            |owned| OrdZSet {
                layer: owned.layer.into_parts().2,
            },
            |borrowed| OrdZSet {
                layer: borrowed.layer.as_parts().2.clone(),
            },
            |_| true,
        );

        streams.insert(node_id, RowStream::Set(set));
    }

    fn fold<C>(&self, node_id: NodeId, fold: Fold, streams: &mut BTreeMap<NodeId, RowStream<C>>)
    where
        C: Circuit,
        C::Time: DBTimestamp,
    {
        let (step_fn, finish_fn) = (fold.step_fn, fold.finish_fn);
        let (acc_vtable, step_vtable, output_vtable) =
            (fold.acc_vtable, fold.step_vtable, fold.output_vtable);

        let folded = match &streams[&fold.input] {
            RowStream::Set(_) => todo!(),

            RowStream::Map(input) => {
                input.aggregate(
                    dbsp::operator::Fold::<_, UnimplementedSemigroup<Row>, _, _>::with_output(
                        fold.init,
                        move |acc: &mut Row, step: &Row, weight| unsafe {
                            debug_assert_eq!(acc.vtable().layout_id, acc_vtable.layout_id);
                            debug_assert_eq!(step.vtable().layout_id, step_vtable.layout_id);

                            step_fn(
                                acc.as_mut_ptr(),
                                step.as_ptr(),
                                &weight as *const i32 as *const u8,
                            );
                        },
                        move |mut acc: Row| unsafe {
                            debug_assert_eq!(acc.vtable().layout_id, acc_vtable.layout_id);

                            let mut row = UninitRow::new(output_vtable);
                            finish_fn(acc.as_mut_ptr(), row.as_mut_ptr());
                            row.assume_init()
                        },
                    ),
                )
            }
        };

        streams.insert(node_id, RowStream::Map(folded));
    }

    fn topk(
        &self,
        node_id: NodeId,
        topk: Topk,
        streams: &mut BTreeMap<NodeId, RowStream<RootCircuit>>,
    ) {
        let k = topk.k.try_into().expect("topk k overflowed a usize");
        let topk = match &streams[&topk.input] {
            RowStream::Set(_input) => {
                // We get an `OrdIndexedZSet<K, ()>` from this, do we have to convert all of the
                // `()` values to unit rows or can we just convert the map into a set (which should
                // be a noop)
                // ```
                // let topk = match topk.order {
                //     TopkOrder::Ascending => input.topk_asc(k),
                //     TopkOrder::Descending => input.topk_desc(k),
                // };
                //
                // RowStream::Set(topk)
                // ```
                todo!("topk set api is kinda funky")
            }

            RowStream::Map(input) => RowStream::Map(match topk.order {
                TopkOrder::Ascending => input.topk_asc(k),
                TopkOrder::Descending => input.topk_desc(k),
            }),
        };

        streams.insert(node_id, topk);
    }
}

#[inline]
fn cast_uninit_vec<T>(vec: Vec<T>) -> Vec<MaybeUninit<T>> {
    // Make sure we don't drop the old vec
    let mut vec = ManuallyDrop::new(vec);

    // Get the length, capacity and pointer of the vec (we get the pointer last as a
    // rather nitpicky thing irt stacked borrows since the `.len()` and
    // `.capacity()` calls technically reborrow the vec). Ideally we'd use
    // `Vec::into_raw_parts()` but it's currently unstable via rust/#65816
    let (len, cap, ptr) = (vec.len(), vec.capacity(), vec.as_mut_ptr());

    // Create a new vec with the different type
    unsafe { Vec::from_raw_parts(ptr.cast::<MaybeUninit<T>>(), len, cap) }
}

fn collect_functions(
    codegen: &mut Codegen,
    functions: &mut BTreeMap<NodeId, Vec<FuncId>>,
    vtables: &mut BTreeMap<LayoutId, LayoutVTable>,
    graph: &graph::Subgraph,
) {
    for (&node_id, node) in graph.nodes() {
        match node {
            Node::Map(map) => {
                let map_fn = codegen.codegen_func(&format!("map_fn_{node_id}"), map.map_fn());
                functions.insert(node_id, vec![map_fn]);

                for layout in [map.input_layout(), map.output_layout()] {
                    match layout {
                        StreamLayout::Set(key) => {
                            vtables
                                .entry(key)
                                .or_insert_with(|| codegen.vtable_for(key));
                        }

                        StreamLayout::Map(key, value) => {
                            vtables
                                .entry(key)
                                .or_insert_with(|| codegen.vtable_for(key));
                            vtables
                                .entry(value)
                                .or_insert_with(|| codegen.vtable_for(value));
                        }
                    }
                }
            }

            Node::Filter(filter) => {
                let filter_fn =
                    codegen.codegen_func(&format!("filter_fn_{node_id}"), filter.filter_fn());
                functions.insert(node_id, vec![filter_fn]);
            }

            Node::FilterMap(filter_map) => {
                let fmap_fn = codegen
                    .codegen_func(&format!("filter_map_fn_{node_id}"), filter_map.filter_map());
                functions.insert(node_id, vec![fmap_fn]);

                vtables
                    .entry(filter_map.layout())
                    .or_insert_with(|| codegen.vtable_for(filter_map.layout()));
            }

            Node::Fold(fold) => {
                let step_fn =
                    codegen.codegen_func(&format!("fold_step_fn_{node_id}"), fold.step_fn());
                let finish_fn =
                    codegen.codegen_func(&format!("fold_finish_fn_{node_id}"), fold.finish_fn());
                functions.insert(node_id, vec![step_fn, finish_fn]);

                for layout in [fold.acc_layout(), fold.step_layout(), fold.output_layout()] {
                    vtables
                        .entry(layout)
                        .or_insert_with(|| codegen.vtable_for(layout));
                }
            }

            Node::PartitionedRollingFold(fold) => {
                let step_fn = codegen.codegen_func(
                    &format!("partitioned_rolling_fold_step_fn_{node_id}"),
                    fold.step_fn(),
                );
                let finish_fn = codegen.codegen_func(
                    &format!("partitioned_rolling_fold_finish_fn_{node_id}"),
                    fold.finish_fn(),
                );
                functions.insert(node_id, vec![step_fn, finish_fn]);

                for layout in [fold.acc_layout(), fold.step_layout(), fold.output_layout()] {
                    vtables
                        .entry(layout)
                        .or_insert_with(|| codegen.vtable_for(layout));
                }
            }

            Node::FlatMap(flat_map) => {
                let flat_map_fn =
                    codegen.codegen_func(&format!("flat_map_fn_{node_id}"), flat_map.flat_map());
                functions.insert(node_id, vec![flat_map_fn]);

                match flat_map.output_layout() {
                    StreamLayout::Set(key) => {
                        vtables
                            .entry(key)
                            .or_insert_with(|| codegen.vtable_for(key));
                    }

                    StreamLayout::Map(key, value) => {
                        vtables
                            .entry(key)
                            .or_insert_with(|| codegen.vtable_for(key));
                        vtables
                            .entry(value)
                            .or_insert_with(|| codegen.vtable_for(value));
                    }
                }
            }

            Node::IndexWith(index_with) => {
                let index_fn =
                    codegen.codegen_func(&format!("index_fn_{node_id}"), index_with.index_fn());
                functions.insert(node_id, vec![index_fn]);

                vtables
                    .entry(index_with.key_layout())
                    .or_insert_with(|| codegen.vtable_for(index_with.key_layout()));
                vtables
                    .entry(index_with.value_layout())
                    .or_insert_with(|| codegen.vtable_for(index_with.value_layout()));
            }

            Node::IndexByColumn(index_by) => {
                let (owned, borrowed) = codegen.codegen_index_by_column(index_by);
                functions.insert(node_id, vec![owned, borrowed]);

                vtables
                    .entry(index_by.key_layout())
                    .or_insert_with(|| codegen.vtable_for(index_by.key_layout()));
                vtables
                    .entry(index_by.value_layout())
                    .or_insert_with(|| codegen.vtable_for(index_by.value_layout()));
            }

            Node::Source(source) => {
                vtables
                    .entry(source.layout())
                    .or_insert_with(|| codegen.vtable_for(source.layout()));
            }

            Node::SourceMap(source) => {
                vtables
                    .entry(source.key())
                    .or_insert_with(|| codegen.vtable_for(source.key()));
                vtables
                    .entry(source.value())
                    .or_insert_with(|| codegen.vtable_for(source.value()));
            }

            Node::JoinCore(join) => {
                let join_fn = codegen.codegen_func(&format!("join_fn_{node_id}"), join.join_fn());
                functions.insert(node_id, vec![join_fn]);

                vtables
                    .entry(join.key_layout())
                    .or_insert_with(|| codegen.vtable_for(join.key_layout()));
                vtables
                    .entry(join.value_layout())
                    .or_insert_with(|| codegen.vtable_for(join.value_layout()));
            }

            Node::MonotonicJoin(join) => {
                let join_fn =
                    codegen.codegen_func(&format!("monotonic_join_fn_{node_id}"), join.join_fn());
                functions.insert(node_id, vec![join_fn]);

                vtables
                    .entry(join.key_layout())
                    .or_insert_with(|| codegen.vtable_for(join.key_layout()));
            }

            Node::Subgraph(subgraph) => {
                collect_functions(codegen, functions, vtables, subgraph.subgraph());
            }

            Node::ConstantStream(constant) => match constant.layout() {
                StreamLayout::Set(key) => {
                    vtables
                        .entry(key)
                        .or_insert_with(|| codegen.vtable_for(key));
                }

                StreamLayout::Map(key, value) => {
                    vtables
                        .entry(key)
                        .or_insert_with(|| codegen.vtable_for(key));
                    vtables
                        .entry(value)
                        .or_insert_with(|| codegen.vtable_for(value));
                }
            },

            Node::UnitMapToSet(map_to_set) => {
                vtables
                    .entry(map_to_set.value_layout())
                    .or_insert_with(|| codegen.vtable_for(map_to_set.value_layout()));
            }

            Node::Min(_)
            | Node::Max(_)
            | Node::Distinct(_)
            | Node::Delta0(_)
            | Node::DelayedFeedback(_)
            | Node::Neg(_)
            | Node::Sum(_)
            | Node::Differentiate(_)
            | Node::Integrate(_)
            | Node::Sink(_)
            | Node::Export(_)
            | Node::ExportedNode(_)
            | Node::Minus(_)
            | Node::Antijoin(_)
            | Node::Topk(_) => {}
        }
    }
}

fn compile_nodes(
    graph: &graph::Subgraph,
    vtables: &BTreeMap<LayoutId, *mut VTable>,
    jit: &JITModule,
    node_streams: &BTreeMap<NodeId, Option<StreamLayout>>,
    node_functions: &BTreeMap<NodeId, Vec<FuncId>>,
    layout_cache: &NativeLayoutCache,
) -> BTreeMap<NodeId, DataflowNode> {
    let mut nodes = BTreeMap::new();
    for (node_id, node) in graph.nodes() {
        let output = node_streams[node_id];

        match node {
            Node::Map(map) => {
                let input = map.input();
                let map_fn = jit.get_finalized_function(node_functions[node_id][0]);

                let map_fn = unsafe {
                    match (map.input_layout(), map.output_layout()) {
                        (StreamLayout::Set(_), StreamLayout::Set(key_layout)) => MapFn::SetSet {
                            map: transmute(map_fn),
                            key_vtable: &*vtables[&key_layout],
                        },

                        (StreamLayout::Set(_), StreamLayout::Map(key_layout, value_layout)) => {
                            MapFn::SetMap {
                                map: transmute(map_fn),
                                key_vtable: &*vtables[&key_layout],
                                value_vtable: &*vtables[&value_layout],
                            }
                        }

                        (StreamLayout::Map(_, _), StreamLayout::Set(key_layout)) => MapFn::MapSet {
                            map: transmute(map_fn),
                            key_vtable: &*vtables[&key_layout],
                        },

                        (StreamLayout::Map(_, _), StreamLayout::Map(key_layout, value_layout)) => {
                            MapFn::MapMap {
                                map: transmute(map_fn),
                                key_vtable: &*vtables[&key_layout],
                                value_vtable: &*vtables[&value_layout],
                            }
                        }
                    }
                };
                let map = DataflowNode::Map(Map { input, map_fn });

                nodes.insert(*node_id, map);
            }

            Node::Filter(filter) => {
                let filter_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let input = filter.input();

                let (filter_fn, layout) = unsafe {
                    match output.unwrap() {
                        StreamLayout::Set(key) => {
                            (FilterFn::Set(transmute(filter_fn)), StreamLayout::Set(key))
                        }
                        StreamLayout::Map(key, value) => (
                            FilterFn::Map(transmute(filter_fn)),
                            StreamLayout::Map(key, value),
                        ),
                    }
                };
                let filter = DataflowNode::Filter(Filter {
                    input,
                    filter_fn,
                    layout,
                });

                nodes.insert(*node_id, filter);
            }

            Node::FilterMap(filter_map) => {
                let fmap_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let input = filter_map.input();
                let output_vtable = unsafe { &*vtables[&filter_map.layout()] };

                let node = match output.unwrap() {
                    StreamLayout::Set(_) => DataflowNode::FilterMap(FilterMap {
                        input,
                        filter_map: unsafe {
                            transmute::<_, unsafe extern "C" fn(*const u8, *mut u8) -> bool>(
                                fmap_fn,
                            )
                        },
                        output_vtable,
                    }),

                    StreamLayout::Map(..) => DataflowNode::FilterMapIndex(FilterMapIndex {
                        input,
                        filter_map: unsafe {
                            transmute::<
                                _,
                                unsafe extern "C" fn(*const u8, *const u8, *mut u8) -> bool,
                            >(fmap_fn)
                        },
                        output_vtable,
                    }),
                };

                nodes.insert(*node_id, node);
            }

            Node::FlatMap(flat_map) => {
                let flat_map_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let input_is_map = node_streams[&flat_map.input()].unwrap().is_map();
                let flat_map = DataflowNode::FlatMap(FlatMap {
                    input: flat_map.input(),
                    flat_map: match flat_map.output_layout() {
                        StreamLayout::Set(key) => {
                            let key_vtable = unsafe { &*vtables[&key] };

                            if input_is_map {
                                FlatMapFn::MapSet {
                                    flat_map: unsafe { transmute(flat_map_fn) },
                                    key_vtable,
                                }
                            } else {
                                FlatMapFn::SetSet {
                                    flat_map: unsafe { transmute(flat_map_fn) },
                                    key_vtable,
                                }
                            }
                        }

                        StreamLayout::Map(key, value) => {
                            let (key_vtable, value_vtable) =
                                unsafe { (&*vtables[&key], &*vtables[&value]) };

                            if input_is_map {
                                FlatMapFn::MapMap {
                                    flat_map: unsafe { transmute(flat_map_fn) },
                                    key_vtable,
                                    value_vtable,
                                }
                            } else {
                                FlatMapFn::SetMap {
                                    flat_map: unsafe { transmute(flat_map_fn) },
                                    key_vtable,
                                    value_vtable,
                                }
                            }
                        }
                    },
                });
                nodes.insert(*node_id, flat_map);
            }

            Node::Neg(neg) => {
                nodes.insert(*node_id, DataflowNode::Neg(Neg { input: neg.input() }));
            }

            Node::Sum(sum) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Sum(Sum {
                        inputs: sum.inputs().to_vec(),
                    }),
                );
            }

            Node::Minus(minus) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Minus(Minus {
                        lhs: minus.lhs(),
                        rhs: minus.rhs(),
                    }),
                );
            }

            Node::Fold(fold) => {
                let (acc_vtable, step_vtable, output_vtable) = unsafe {
                    (
                        &*vtables[&fold.acc_layout()],
                        &*vtables[&fold.step_layout()],
                        &*vtables[&fold.output_layout()],
                    )
                };
                let acc_layout = layout_cache.layout_of(fold.acc_layout());

                let init = unsafe { row_from_literal(fold.init(), acc_vtable, &acc_layout) };

                let (step_fn, finish_fn) = (
                    jit.get_finalized_function(node_functions[node_id][0]),
                    jit.get_finalized_function(node_functions[node_id][1]),
                );

                let fold = DataflowNode::Fold(Fold {
                    input: fold.input(),
                    init,
                    acc_vtable,
                    step_vtable,
                    output_vtable,
                    step_fn: unsafe { transmute(step_fn) },
                    finish_fn: unsafe { transmute(finish_fn) },
                });
                nodes.insert(*node_id, fold);
            }

            Node::PartitionedRollingFold(fold) => {
                let (acc_vtable, step_vtable, output_vtable) = unsafe {
                    (
                        &*vtables[&fold.acc_layout()],
                        &*vtables[&fold.step_layout()],
                        &*vtables[&fold.output_layout()],
                    )
                };
                let acc_layout = layout_cache.layout_of(fold.acc_layout());

                let init = unsafe { row_from_literal(fold.init(), acc_vtable, &acc_layout) };

                let (step_fn, finish_fn) = (
                    jit.get_finalized_function(node_functions[node_id][0]),
                    jit.get_finalized_function(node_functions[node_id][1]),
                );

                let fold = DataflowNode::PartitionedRollingFold(PartitionedRollingFold {
                    input: fold.input(),
                    range: fold.range(),
                    init,
                    acc_vtable,
                    step_vtable,
                    output_vtable,
                    step_fn: unsafe { transmute(step_fn) },
                    finish_fn: unsafe { transmute(finish_fn) },
                });
                nodes.insert(*node_id, fold);
            }

            Node::Sink(sink) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Sink(Sink {
                        input: sink.input(),
                        layout: node_streams[&sink.input()].unwrap(),
                    }),
                );
            }

            Node::Source(source) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Source(Source {
                        key_layout: source.layout(),
                    }),
                );
            }

            Node::SourceMap(source) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::SourceMap(SourceMap {
                        key_layout: source.key(),
                        value_layout: source.value(),
                    }),
                );
            }

            Node::IndexWith(index) => {
                let input = index.input();
                let index_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let key_vtable = unsafe { &*vtables[&index.key_layout()] };
                let value_vtable = unsafe { &*vtables[&index.value_layout()] };

                let node = match node_streams[&input].unwrap() {
                    StreamLayout::Set(_) => DataflowNode::IndexWith(IndexWith {
                        input,
                        index_fn: unsafe {
                            transmute::<_, unsafe extern "C" fn(*const u8, *mut u8, *mut u8)>(
                                index_fn,
                            )
                        },
                        key_vtable,
                        value_vtable,
                    }),

                    StreamLayout::Map(..) => todo!(),
                };

                nodes.insert(*node_id, node);
            }

            Node::IndexByColumn(index_by) => {
                let input = index_by.input();
                let owned_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let borrowed_fn = jit.get_finalized_function(node_functions[node_id][1]);
                let key_vtable = unsafe { &*vtables[&index_by.key_layout()] };
                let value_vtable = unsafe { &*vtables[&index_by.value_layout()] };

                let node = DataflowNode::IndexByColumn(IndexByColumn {
                    input,
                    owned_fn: unsafe { transmute(owned_fn) },
                    borrowed_fn: unsafe { transmute(borrowed_fn) },
                    key_vtable,
                    value_vtable,
                });

                nodes.insert(*node_id, node);
            }

            Node::UnitMapToSet(map_to_set) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::UnitMapToSet(UnitMapToSet {
                        input: map_to_set.input(),
                    }),
                );
            }

            Node::Differentiate(diff) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Differentiate(Differentiate {
                        input: diff.input(),
                    }),
                );
            }

            Node::Integrate(integrate) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Integrate(Integrate {
                        input: integrate.input(),
                    }),
                );
            }

            Node::Delta0(delta) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Delta0(Delta0 {
                        input: delta.input(),
                    }),
                );
            }

            Node::DelayedFeedback(_) => {
                nodes.insert(*node_id, DataflowNode::DelayedFeedback(DelayedFeedback {}));
            }

            Node::Min(min) => {
                nodes.insert(*node_id, DataflowNode::Min(Min { input: min.input() }));
            }

            Node::Max(max) => {
                nodes.insert(*node_id, DataflowNode::Max(Max { input: max.input() }));
            }

            Node::Distinct(distinct) => {
                nodes.insert(
                    *node_id,
                    DataflowNode::Distinct(Distinct {
                        input: distinct.input(),
                    }),
                );
            }

            Node::JoinCore(join) => {
                let (lhs, rhs) = (join.lhs(), join.rhs());
                let join_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let key_vtable = unsafe { &*vtables[&join.key_layout()] };
                let value_vtable = unsafe { &*vtables[&join.value_layout()] };

                let node = match (node_streams[&lhs].unwrap(), node_streams[&rhs].unwrap()) {
                    (StreamLayout::Map(..), StreamLayout::Map(..)) => {
                        DataflowNode::JoinCore(JoinCore {
                            lhs,
                            rhs,
                            join_fn: unsafe {
                                transmute::<
                                    _,
                                    unsafe extern "C" fn(
                                        *const u8,
                                        *const u8,
                                        *const u8,
                                        *mut u8,
                                        *mut u8,
                                    ),
                                >(join_fn)
                            },
                            key_vtable,
                            value_vtable,
                            output_kind: join.result_kind(),
                        })
                    }

                    _ => todo!(),
                };

                nodes.insert(*node_id, node);
            }

            Node::MonotonicJoin(join) => {
                let (lhs, rhs) = (join.lhs(), join.rhs());
                let join_fn = jit.get_finalized_function(node_functions[node_id][0]);
                let key_vtable = unsafe { &*vtables[&join.key_layout()] };

                let node = DataflowNode::MonotonicJoin(MonotonicJoin {
                    lhs,
                    rhs,
                    join_fn: unsafe {
                        transmute::<_, unsafe extern "C" fn(*const u8, *const u8, *const u8, *mut u8)>(
                            join_fn,
                        )
                    },
                    key_vtable,
                });
                nodes.insert(*node_id, node);
            }

            Node::Antijoin(antijoin) => {
                let node = DataflowNode::Antijoin(Antijoin {
                    lhs: antijoin.lhs(),
                    rhs: antijoin.rhs(),
                });
                nodes.insert(*node_id, node);
            }

            Node::Topk(topk) => {
                let node = DataflowNode::Topk(Topk {
                    input: topk.input(),
                    k: topk.k(),
                    order: topk.order(),
                });
                nodes.insert(*node_id, node);
            }

            Node::Subgraph(subgraph) => {
                let node = DataflowNode::Subgraph(DataflowSubgraph {
                    edges: subgraph.edges().clone(),
                    inputs: subgraph.input_nodes().clone(),
                    nodes: compile_nodes(
                        subgraph.subgraph(),
                        vtables,
                        jit,
                        node_streams,
                        node_functions,
                        layout_cache,
                    ),
                    feedback_connections: subgraph.feedback_connections().clone(),
                });
                nodes.insert(*node_id, node);
            }

            Node::Export(export) => {
                let node = DataflowNode::Export(Export {
                    input: export.input(),
                });
                nodes.insert(*node_id, node);
            }

            Node::ExportedNode(exported) => {
                let node = DataflowNode::Noop(Noop {
                    input: exported.input(),
                });
                nodes.insert(*node_id, node);
            }

            Node::ConstantStream(constant) => {
                let value = match constant.value().value() {
                    StreamCollection::Set(set) => {
                        let key_layout = constant.layout().unwrap_set();
                        let key_vtable = unsafe { &*vtables[&key_layout] };
                        let key_layout = layout_cache.layout_of(key_layout);

                        let mut batch = Vec::with_capacity(set.len());
                        for (literal, diff) in set {
                            let key = unsafe { row_from_literal(literal, key_vtable, &key_layout) };

                            batch.push((key, *diff));
                        }

                        // Build a batch from the set's values
                        let mut batcher = <RowSet as Batch>::Batcher::new_batcher(());
                        batcher.push_batch(&mut batch);
                        RowZSet::Set(batcher.seal())
                    }

                    StreamCollection::Map(map) => {
                        let (key_layout, value_layout) = constant.layout().unwrap_map();
                        let (key_vtable, value_vtable) =
                            unsafe { (&*vtables[&key_layout], &*vtables[&value_layout]) };
                        let (key_layout, value_layout) = (
                            layout_cache.layout_of(key_layout),
                            layout_cache.layout_of(value_layout),
                        );

                        let mut batch = Vec::with_capacity(map.len());
                        for (key_literal, value_literal, diff) in map {
                            let key =
                                unsafe { row_from_literal(key_literal, key_vtable, &key_layout) };
                            let value = unsafe {
                                row_from_literal(value_literal, value_vtable, &value_layout)
                            };

                            batch.push(((key, value), *diff));
                        }

                        // Build a batch from the set's values
                        let mut batcher = <RowMap as Batch>::Batcher::new_batcher(());
                        batcher.push_batch(&mut batch);
                        RowZSet::Map(batcher.seal())
                    }
                };

                let node = DataflowNode::Constant(nodes::Constant { value });
                nodes.insert(*node_id, node);
            }
        }
    }

    nodes
}
