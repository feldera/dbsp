use crate::ir::{
    layout_cache::RowLayoutCache,
    literal::StreamLiteral,
    nodes::{DataflowNode, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId, NodeId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
pub struct ConstantStream {
    value: StreamLiteral,
    #[serde(default)]
    consolidated: bool,
}

impl ConstantStream {
    pub const fn new(value: StreamLiteral) -> Self {
        Self {
            value,
            consolidated: false,
        }
    }

    /// Create an empty stream
    pub const fn empty(layout: StreamLayout) -> Self {
        Self::new(StreamLiteral::empty(layout))
    }

    pub const fn value(&self) -> &StreamLiteral {
        &self.value
    }

    pub fn value_mut(&mut self) -> &mut StreamLiteral {
        &mut self.value
    }

    pub const fn layout(&self) -> StreamLayout {
        self.value.layout()
    }

    pub const fn consolidated(&self) -> bool {
        self.consolidated
    }

    pub fn consolidate(&mut self) {
        if !self.consolidated {
            let start_len = self.value.len();
            self.value.consolidate();

            let removed = start_len - self.value.len();
            if removed != 0 {
                tracing::trace!("removed {removed} items from constant stream");
            }

            self.consolidated = true;
        }
    }
}

impl DataflowNode for ConstantStream {
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
        Some(self.layout())
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {
        self.consolidate();
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.value.layout().map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.value.layout_mut().remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &ConstantStream
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("constant")
            .append(alloc.space())
            .append(self.layout().pretty(alloc, cache))
            .append(alloc.space())
            .append(self.value.value().pretty(alloc, cache))
    }
}
