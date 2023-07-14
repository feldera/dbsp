use crate::ir::{
    layout_cache::RowLayoutCache,
    nodes::{DataflowNode, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId, NodeId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Sorts and retains the top `k` elements of the input stream
///
/// If the input stream is a `Set<K>` then the output stream will be a
/// `Map<K, ()>`
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TopK {
    /// The input stream
    input: NodeId,
    /// The layout of the input stream
    layout: StreamLayout,
    /// The number of elements to retain
    k: u64,
    /// The order to sort items by
    order: TopkOrder,
}

impl TopK {
    pub fn new(input: NodeId, layout: StreamLayout, k: u64, order: TopkOrder) -> Self {
        Self {
            input,
            layout,
            k,
            order,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn layout(&self) -> StreamLayout {
        self.layout
    }

    pub const fn k(&self) -> u64 {
        self.k
    }

    pub const fn order(&self) -> TopkOrder {
        self.order
    }

    pub fn output_layout(&self, cache: &RowLayoutCache) -> StreamLayout {
        match self.layout {
            StreamLayout::Set(key) => StreamLayout::Map(cache.unit(), key),
            layout @ StreamLayout::Map(..) => layout,
        }
    }
}

impl DataflowNode for TopK {
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
        layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.output_layout(layout_cache))
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

impl<'a, D, A> Pretty<'a, D, A> for &TopK
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("topk")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.order.pretty(alloc, cache))
            .append(alloc.space())
            .append(alloc.text(self.k.to_string()))
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema,
)]
pub enum TopkOrder {
    Ascending,
    Descending,
}

impl TopkOrder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ascending => "asc",
            Self::Descending => "desc",
        }
    }
}

impl<'a, D, A> Pretty<'a, D, A> for TopkOrder
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, _cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc.text(self.as_str())
    }
}
