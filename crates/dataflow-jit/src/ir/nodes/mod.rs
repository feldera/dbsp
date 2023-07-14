mod aggregate;
mod constant;
mod differentiate;
mod filter_map;
mod flat_map;
mod index;
mod io;
mod join;
mod subgraph;
mod sum;
mod topk;

pub use crate::ir::{
    stream_layout::{StreamKind, StreamLayout},
    NodeId,
};
pub use aggregate::{Fold, Max, Min, PartitionedRollingFold};
pub use constant::ConstantStream;
pub use differentiate::{Differentiate, Integrate};
pub use filter_map::{Filter, FilterMap, Map};
pub use flat_map::FlatMap;
pub use index::{IndexByColumn, IndexWith, UnitMapToSet};
pub use io::{Export, ExportedNode, Sink, Source, SourceMap};
pub use join::{Antijoin, JoinCore, MonotonicJoin};
pub use subgraph::Subgraph;
pub use sum::{Minus, Sum};
pub use topk::{TopK, TopkOrder};

use crate::ir::{
    function::Function,
    layout_cache::RowLayoutCache,
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId,
};
use derive_more::{IsVariant, Unwrap};
use enum_dispatch::enum_dispatch;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[enum_dispatch(DataflowNode)]
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, IsVariant, Unwrap)]
pub enum Node {
    Map(Map),
    Min(Min),
    Max(Max),
    Neg(Neg),
    Sum(Sum),
    Fold(Fold),
    Sink(Sink),
    Minus(Minus),
    Filter(Filter),
    FilterMap(FilterMap),
    Source(Source),
    SourceMap(SourceMap),
    IndexWith(IndexWith),
    Differentiate(Differentiate),
    Integrate(Integrate),
    Delta0(Delta0),
    DelayedFeedback(DelayedFeedback),
    Distinct(Distinct),
    JoinCore(JoinCore),
    Subgraph(Subgraph),
    Export(Export),
    ExportedNode(ExportedNode),
    MonotonicJoin(MonotonicJoin),
    ConstantStream(ConstantStream),
    PartitionedRollingFold(PartitionedRollingFold),
    FlatMap(FlatMap),
    Antijoin(Antijoin),
    IndexByColumn(IndexByColumn),
    UnitMapToSet(UnitMapToSet),
    Topk(TopK),
    // TODO: OrderBy, Windows
}

impl Node {
    pub const fn as_constant(&self) -> Option<&ConstantStream> {
        if let Self::ConstantStream(constant) = self {
            Some(constant)
        } else {
            None
        }
    }

    pub const fn as_antijoin(&self) -> Option<&Antijoin> {
        if let Self::Antijoin(antijoin) = self {
            Some(antijoin)
        } else {
            None
        }
    }

    pub const fn as_sink(&self) -> Option<&Sink> {
        if let Self::Sink(sink) = self {
            Some(sink)
        } else {
            None
        }
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Node
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        match self {
            Node::Map(map) => map.pretty(alloc, cache),
            Node::Min(min) => min.pretty(alloc, cache),
            Node::Max(max) => max.pretty(alloc, cache),
            Node::Neg(neg) => neg.pretty(alloc, cache),
            Node::Sum(sum) => sum.pretty(alloc, cache),
            Node::Fold(fold) => fold.pretty(alloc, cache),
            Node::Sink(sink) => sink.pretty(alloc, cache),
            Node::Minus(minus) => minus.pretty(alloc, cache),
            Node::Filter(filter) => filter.pretty(alloc, cache),
            Node::FilterMap(filter_map) => filter_map.pretty(alloc, cache),
            Node::Source(source) => source.pretty(alloc, cache),
            Node::SourceMap(source_map) => source_map.pretty(alloc, cache),
            Node::IndexWith(index_with) => index_with.pretty(alloc, cache),
            Node::Differentiate(differentiate) => differentiate.pretty(alloc, cache),
            Node::Integrate(integrate) => integrate.pretty(alloc, cache),
            Node::Delta0(delta0) => delta0.pretty(alloc, cache),
            Node::DelayedFeedback(delayed_feedback) => delayed_feedback.pretty(alloc, cache),
            Node::Distinct(distinct) => distinct.pretty(alloc, cache),
            Node::JoinCore(join_core) => join_core.pretty(alloc, cache),
            Node::MonotonicJoin(monotonic_join) => monotonic_join.pretty(alloc, cache),
            Node::ConstantStream(constant) => constant.pretty(alloc, cache),
            Node::PartitionedRollingFold(rolling_fold) => rolling_fold.pretty(alloc, cache),
            Node::FlatMap(flat_map) => flat_map.pretty(alloc, cache),
            Node::Antijoin(antijoin) => antijoin.pretty(alloc, cache),
            Node::IndexByColumn(index_by_column) => index_by_column.pretty(alloc, cache),
            Node::UnitMapToSet(unit_map_to_set) => unit_map_to_set.pretty(alloc, cache),
            Node::Topk(topk) => topk.pretty(alloc, cache),

            Node::Subgraph(_) | Node::Export(_) | Node::ExportedNode(_) => alloc.nil(),
            // Node::Subgraph(subgraph) => subgraph.pretty(alloc, cache),
            // Node::Export(export) => export.pretty(alloc, cache),
            // Node::ExportedNode(exported_node) => exported_node.pretty(alloc, cache),
        }
    }
}

// TODO: Fully flesh this out, make it useful
#[enum_dispatch]
pub trait DataflowNode {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized;

    fn inputs(&self, inputs: &mut Vec<NodeId>) {
        self.map_inputs(&mut |node_id| inputs.push(node_id));
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized;

    fn output_stream(
        &self,
        inputs: &[StreamLayout],
        layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout>;

    fn output_kind(
        &self,
        inputs: &[StreamLayout],
        layout_cache: &RowLayoutCache,
    ) -> Option<StreamKind> {
        self.output_stream(inputs, layout_cache)
            .map(StreamLayout::kind)
    }

    fn validate(&self, inputs: &[StreamLayout], layout_cache: &RowLayoutCache);

    fn optimize(&mut self, layout_cache: &RowLayoutCache);

    fn functions<'a>(&'a self, _functions: &mut Vec<&'a Function>) {}

    fn functions_mut<'a>(&'a mut self, _functions: &mut Vec<&'a mut Function>) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized;

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>);
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
pub struct Distinct {
    input: NodeId,
    layout: StreamLayout,
}

impl Distinct {
    pub const fn new(input: NodeId, layout: StreamLayout) -> Self {
        Self { input, layout }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for Distinct {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        map(self.input);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        map(&mut self.input);
    }

    fn output_stream(
        &self,
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.layout)
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], self.layout);
    }

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.layout.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.layout.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Distinct
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("distinct")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}

// FIXME: DelayedFeedback with maps
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct DelayedFeedback {
    layout: LayoutId,
}

impl DelayedFeedback {
    pub const fn new(layout: LayoutId) -> Self {
        Self { layout }
    }

    pub const fn layout(&self) -> LayoutId {
        self.layout
    }
}

impl DataflowNode for DelayedFeedback {
    fn map_inputs<F>(&self, _map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
    }

    fn map_inputs_mut<F>(&mut self, _map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
    }

    fn output_stream(
        &self,
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(StreamLayout::Set(self.layout))
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.layout);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.layout = mappings[&self.layout];
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &DelayedFeedback
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("delayed_feedback")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Delta0 {
    input: NodeId,
    layout: StreamLayout,
}

impl Delta0 {
    pub const fn new(input: NodeId, layout: StreamLayout) -> Self {
        Self { input, layout }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for Delta0 {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        map(self.input);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        map(&mut self.input);
    }

    fn output_stream(
        &self,
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.layout)
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], self.layout);
    }

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.layout.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.layout.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Delta0
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("delta0")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Neg {
    input: NodeId,
    layout: StreamLayout,
}

impl Neg {
    pub fn new(input: NodeId, layout: StreamLayout) -> Self {
        Self { input, layout }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for Neg {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        map(self.input);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        map(&mut self.input);
    }

    fn output_stream(
        &self,
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.layout)
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.layout.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.layout.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Neg
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("neg")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}
