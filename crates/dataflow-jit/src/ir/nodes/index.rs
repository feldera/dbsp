use crate::ir::{
    function::Function,
    layout_cache::RowLayoutCache,
    nodes::{DataflowNode, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId, NodeId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct IndexWith {
    input: NodeId,
    /// Expects a function with a signature of `fn(input_layout, mut key_layout,
    /// mut value_layout)`
    index_fn: Function,
    key_layout: LayoutId,
    value_layout: LayoutId,
}

impl IndexWith {
    pub fn new(
        input: NodeId,
        index_fn: Function,
        key_layout: LayoutId,
        value_layout: LayoutId,
    ) -> Self {
        Self {
            input,
            index_fn,
            key_layout,
            value_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn index_fn(&self) -> &Function {
        &self.index_fn
    }

    pub const fn key_layout(&self) -> LayoutId {
        self.key_layout
    }

    pub const fn value_layout(&self) -> LayoutId {
        self.value_layout
    }

    pub const fn output_layout(&self) -> StreamLayout {
        StreamLayout::Map(self.key_layout, self.value_layout)
    }
}

impl DataflowNode for IndexWith {
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
        Some(self.output_layout())
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        // TODO
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.index_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(&self.index_fn);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.index_fn);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.key_layout);
        map(self.value_layout);
        self.index_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.key_layout = mappings[&self.key_layout];
        self.value_layout = mappings[&self.value_layout];
        self.index_fn.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &IndexWith
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("index_with")
            .append(alloc.space())
            .append(self.output_layout().pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("index_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.index_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}

/// Creates an index from an input set, using the `key_column`th column of the
/// input as the index's key and discarding all columns within
/// `discarded_values` in the values field (with the implicit addition of
/// `key_column`, which will never appear in the output value)
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct IndexByColumn {
    input: NodeId,
    input_layout: LayoutId,
    /// The key column
    key_column: usize,
    /// Values to be discarded
    discarded_values: Vec<usize>,
    key_layout: LayoutId,
    value_layout: LayoutId,
}

impl IndexByColumn {
    pub const fn new(
        input: NodeId,
        input_layout: LayoutId,
        key_column: usize,
        discarded_values: Vec<usize>,
        key_layout: LayoutId,
        value_layout: LayoutId,
    ) -> Self {
        Self {
            input,
            input_layout,
            key_column,
            discarded_values,
            key_layout,
            value_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn input_layout(&self) -> LayoutId {
        self.input_layout
    }

    pub const fn key_column(&self) -> usize {
        self.key_column
    }

    pub fn discarded_values(&self) -> &[usize] {
        &self.discarded_values
    }

    pub const fn key_layout(&self) -> LayoutId {
        self.key_layout
    }

    pub const fn value_layout(&self) -> LayoutId {
        self.value_layout
    }

    pub const fn output_layout(&self) -> StreamLayout {
        StreamLayout::Map(self.key_layout, self.value_layout)
    }
}

impl DataflowNode for IndexByColumn {
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
        Some(self.output_layout())
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], StreamLayout::Set(self.input_layout));
    }

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn functions<'a>(&'a self, _functions: &mut Vec<&'a Function>) {}

    fn functions_mut<'a>(&'a mut self, _functions: &mut Vec<&'a mut Function>) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.input_layout);
        map(self.key_layout);
        map(self.value_layout);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.input_layout = mappings[&self.input_layout];
        self.key_layout = mappings[&self.key_layout];
        self.value_layout = mappings[&self.value_layout];
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &IndexByColumn
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("index_by_column")
            .append(alloc.space())
            .append(self.input_layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text(format!("{}", self.key_column)))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(
                alloc
                    .intersperse(
                        self.discarded_values
                            .iter()
                            .map(|column| alloc.text(format!("{column}"))),
                        alloc.text(",").append(alloc.space()),
                    )
                    .brackets(),
            )
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.output_layout().pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct UnitMapToSet {
    input: NodeId,
    value_layout: LayoutId,
}

impl UnitMapToSet {
    pub const fn new(input: NodeId, value_layout: LayoutId) -> Self {
        Self {
            input,
            value_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn value_layout(&self) -> LayoutId {
        self.value_layout
    }
}

impl DataflowNode for UnitMapToSet {
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
        Some(StreamLayout::Set(self.value_layout))
    }

    fn validate(&self, inputs: &[StreamLayout], layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert_eq!(
            inputs[0],
            StreamLayout::Map(layout_cache.unit(), self.value_layout)
        );
    }

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {}

    fn functions<'a>(&'a self, _functions: &mut Vec<&'a Function>) {}

    fn functions_mut<'a>(&'a mut self, _functions: &mut Vec<&'a mut Function>) {}

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.value_layout);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.value_layout = mappings[&self.value_layout];
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &UnitMapToSet
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("unit_map_to_set")
            .append(alloc.space())
            .append(self.value_layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}
