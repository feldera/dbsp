use crate::ir::{
    function::Function,
    layout_cache::RowLayoutCache,
    nodes::{DataflowNode, StreamKind, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId, NodeId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct JoinCore {
    lhs: NodeId,
    rhs: NodeId,
    // fn(key, lhs_val, rhs_val, key_out, val_out)
    join_fn: Function,
    key_layout: LayoutId,
    value_layout: LayoutId,
    output_kind: StreamKind,
}

impl JoinCore {
    pub const fn new(
        lhs: NodeId,
        rhs: NodeId,
        join_fn: Function,
        key_layout: LayoutId,
        value_layout: LayoutId,
        output_kind: StreamKind,
    ) -> Self {
        Self {
            lhs,
            rhs,
            join_fn,
            key_layout,
            value_layout,
            output_kind,
        }
    }

    pub const fn lhs(&self) -> NodeId {
        self.lhs
    }

    pub const fn rhs(&self) -> NodeId {
        self.rhs
    }

    pub const fn join_fn(&self) -> &Function {
        &self.join_fn
    }

    pub const fn key_layout(&self) -> LayoutId {
        self.key_layout
    }

    pub const fn value_layout(&self) -> LayoutId {
        self.value_layout
    }

    pub const fn output_layout(&self) -> StreamLayout {
        match self.output_kind {
            StreamKind::Set => StreamLayout::Set(self.key_layout),
            StreamKind::Map => StreamLayout::Map(self.key_layout, self.value_layout),
        }
    }

    pub(crate) fn result_kind(&self) -> StreamKind {
        self.output_kind
    }
}

impl DataflowNode for JoinCore {
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
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.output_layout())
    }

    fn validate(&self, _inputs: &[StreamLayout], layout_cache: &RowLayoutCache) {
        if self.output_kind.is_set() {
            assert_eq!(self.value_layout, layout_cache.unit());
        }
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.join_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(&self.join_fn);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.join_fn);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.key_layout);
        map(self.value_layout);
        self.join_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.key_layout = mappings[&self.key_layout];
        self.value_layout = mappings[&self.value_layout];
        self.join_fn.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &JoinCore
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("join_core")
            .append(alloc.space())
            .append(self.output_layout().pretty(alloc, cache))
            .append(alloc.space())
            .append(self.lhs.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.rhs.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("join_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.join_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MonotonicJoin {
    lhs: NodeId,
    rhs: NodeId,
    // fn(key, lhs_val, rhs_val, key_out)
    join_fn: Function,
    key_layout: LayoutId,
}

impl MonotonicJoin {
    pub const fn new(lhs: NodeId, rhs: NodeId, join_fn: Function, key_layout: LayoutId) -> Self {
        Self {
            lhs,
            rhs,
            join_fn,
            key_layout,
        }
    }

    pub const fn lhs(&self) -> NodeId {
        self.lhs
    }

    pub const fn rhs(&self) -> NodeId {
        self.rhs
    }

    pub const fn join_fn(&self) -> &Function {
        &self.join_fn
    }

    pub const fn key_layout(&self) -> LayoutId {
        self.key_layout
    }
}

impl DataflowNode for MonotonicJoin {
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
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(StreamLayout::Set(self.key_layout))
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.join_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(&self.join_fn);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.join_fn);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.key_layout);
        self.join_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.key_layout = mappings[&self.key_layout];
        self.join_fn.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &MonotonicJoin
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("monotonic_join")
            .append(alloc.space())
            .append(self.lhs.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.rhs.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("join_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.join_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Antijoin {
    lhs: NodeId,
    rhs: NodeId,
    layout: StreamLayout,
}

impl Antijoin {
    pub const fn new(lhs: NodeId, rhs: NodeId, layout: StreamLayout) -> Self {
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

impl DataflowNode for Antijoin {
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
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(self.layout)
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn functions<'a>(&'a self, _functions: &mut Vec<&'a Function>) {}

    fn functions_mut<'a>(&'a mut self, _functions: &mut Vec<&'a mut Function>) {}

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

impl<'a, D, A> Pretty<'a, D, A> for &Antijoin
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("integrate")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.lhs.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.rhs.pretty(alloc, cache))
    }
}
