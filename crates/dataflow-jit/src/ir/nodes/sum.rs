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
pub struct Sum {
    inputs: Vec<NodeId>,
    layout: StreamLayout,
}

impl Sum {
    pub fn new(inputs: Vec<NodeId>, layout: StreamLayout) -> Self {
        Self { inputs, layout }
    }

    pub fn inputs(&self) -> &[NodeId] {
        &self.inputs
    }

    pub fn inputs_mut(&mut self) -> &mut Vec<NodeId> {
        &mut self.inputs
    }

    pub const fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for Sum {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        self.inputs.iter().copied().for_each(map);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        self.inputs.iter_mut().for_each(map);
    }

    fn output_stream(
        &self,
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.layout)
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert!(inputs.iter().all(|&layout| layout == self.layout));
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

impl<'a, D, A> Pretty<'a, D, A> for &Sum
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("sum")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(
                alloc
                    .intersperse(
                        self.inputs.iter().map(|input| input.pretty(alloc, cache)),
                        alloc.text(",").append(alloc.space()),
                    )
                    .brackets(),
            )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Minus {
    lhs: NodeId,
    rhs: NodeId,
    layout: StreamLayout,
}

impl Minus {
    pub fn new(lhs: NodeId, rhs: NodeId, layout: StreamLayout) -> Self {
        Self { lhs, rhs, layout }
    }

    pub const fn lhs(&self) -> NodeId {
        self.lhs
    }

    pub const fn rhs(&self) -> NodeId {
        self.rhs
    }

    pub const fn layout(&self) -> StreamLayout {
        self.layout
    }
}

impl DataflowNode for Minus {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        map(self.lhs);
        map(self.rhs);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        map(&mut self.lhs);
        map(&mut self.rhs);
    }

    fn output_stream(
        &self,
        inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(inputs[0])
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0], inputs[1]);
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

impl<'a, D, A> Pretty<'a, D, A> for &Minus
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("minus")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.lhs.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.lhs.pretty(alloc, cache))
    }
}
