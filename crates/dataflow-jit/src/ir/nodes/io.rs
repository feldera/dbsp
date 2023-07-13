use crate::ir::{
    layout_cache::RowLayoutCache,
    nodes::{DataflowNode, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId, NodeId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Export {
    input: NodeId,
    layout: StreamLayout,
}

impl Export {
    pub fn new(input: NodeId, layout: StreamLayout) -> Self {
        Self { input, layout }
    }

    pub fn input(&self) -> NodeId {
        self.input
    }

    pub fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for Export {
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

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ExportedNode {
    subgraph: NodeId,
    input: NodeId,
    layout: StreamLayout,
}

impl ExportedNode {
    pub fn new(subgraph: NodeId, input: NodeId, layout: StreamLayout) -> Self {
        Self {
            subgraph,
            input,
            layout,
        }
    }

    pub fn subgraph(&self) -> NodeId {
        self.subgraph
    }

    pub fn input(&self) -> NodeId {
        self.input
    }

    pub fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for ExportedNode {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        map(self.subgraph);
        map(self.input);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        map(&mut self.subgraph);
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

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Source {
    /// The type of the source's produced stream
    layout: LayoutId,
}

impl Source {
    pub const fn new(layout: LayoutId) -> Self {
        Self { layout }
    }

    /// The type of the source's produced stream
    pub const fn layout(&self) -> LayoutId {
        self.layout
    }

    pub const fn output_layout(&self) -> StreamLayout {
        StreamLayout::Set(self.layout)
    }
}

impl DataflowNode for Source {
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

impl<'a, D, A> Pretty<'a, D, A> for &Source
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("source")
            .append(alloc.space())
            .append(StreamLayout::Set(self.layout).pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SourceMap {
    key_layout: LayoutId,
    value_layout: LayoutId,
}

impl SourceMap {
    pub const fn new(key: LayoutId, value: LayoutId) -> Self {
        Self {
            key_layout: key,
            value_layout: value,
        }
    }

    /// The key type of the source's produced stream
    pub const fn key(&self) -> LayoutId {
        self.key_layout
    }

    /// The value type of the source's produced stream
    pub const fn value(&self) -> LayoutId {
        self.value_layout
    }

    pub const fn output_layout(&self) -> StreamLayout {
        StreamLayout::Map(self.key_layout, self.value_layout)
    }
}

impl DataflowNode for SourceMap {
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
        Some(self.output_layout())
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.key_layout);
        map(self.value_layout);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.key_layout = mappings[&self.key_layout];
        self.value_layout = mappings[&self.value_layout];
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &SourceMap
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("source_map")
            .append(alloc.space())
            .append(StreamLayout::Map(self.key_layout, self.value_layout).pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Sink {
    input: NodeId,
    input_layout: StreamLayout,
}

impl Sink {
    pub fn new(input: NodeId, input_layout: StreamLayout) -> Self {
        Self {
            input,
            input_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn input_layout(&self) -> StreamLayout {
        self.input_layout
    }
}

impl DataflowNode for Sink {
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
        None
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert_eq!(self.input_layout, inputs[0]);
    }

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.input_layout.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.input_layout.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Sink
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("sink")
            .append(alloc.space())
            .append(self.input_layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}
