use crate::ir::{
    function::{Function, InputFlags},
    layout_cache::RowLayoutCache,
    nodes::{DataflowNode, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    LayoutId, NodeId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Map {
    input: NodeId,
    map_fn: Function,
    input_layout: StreamLayout,
    output_layout: StreamLayout,
}

impl Map {
    pub const fn new(
        input: NodeId,
        map_fn: Function,
        input_layout: StreamLayout,
        output_layout: StreamLayout,
    ) -> Self {
        Self {
            input,
            map_fn,
            input_layout,
            output_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn map_fn(&self) -> &Function {
        &self.map_fn
    }

    pub const fn input_layout(&self) -> StreamLayout {
        self.input_layout
    }

    pub const fn output_layout(&self) -> StreamLayout {
        self.output_layout
    }
}

impl DataflowNode for Map {
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
        Some(self.output_layout)
    }

    fn validate(&self, inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], self.input_layout);

        let expected = match (self.input_layout, self.output_layout) {
            (StreamLayout::Set(input), StreamLayout::Set(output)) => {
                vec![(input, InputFlags::INPUT), (output, InputFlags::OUTPUT)]
            }

            (StreamLayout::Set(input), StreamLayout::Map(output_key, output_value)) => vec![
                (input, InputFlags::INPUT),
                (output_key, InputFlags::OUTPUT),
                (output_value, InputFlags::OUTPUT),
            ],

            (StreamLayout::Map(input_key, input_value), StreamLayout::Set(output)) => vec![
                (input_key, InputFlags::INPUT),
                (input_value, InputFlags::INPUT),
                (output, InputFlags::OUTPUT),
            ],

            (
                StreamLayout::Map(input_key, input_value),
                StreamLayout::Map(output_key, output_value),
            ) => vec![
                (input_key, InputFlags::INPUT),
                (input_value, InputFlags::INPUT),
                (output_key, InputFlags::OUTPUT),
                (output_value, InputFlags::OUTPUT),
            ],
        };

        assert_eq!(
            self.map_fn.args().len(),
            expected.len(),
            "got {} map function arguments, expected {}",
            self.map_fn.args().len(),
            expected.len(),
        );

        for (idx, (arg, (expected_layout, expected_flags))) in
            self.map_fn.args().iter().zip(expected).enumerate()
        {
            if arg.layout != expected_layout {
                panic!(
                    "expected map argument #{} to have layout {expected_layout} but it has the layout {}",
                    idx + 1,
                    arg.layout,
                );
            }

            if arg.flags != expected_flags {
                panic!(
                    "expected map argument #{} to have flags {} but it has the flags {}",
                    idx + 1,
                    expected_flags.to_str(),
                    arg.flags.to_str(),
                );
            }
        }

        assert!(
            self.map_fn.return_type().is_unit(),
            "map function is expected to have a unit return type but it returned {}",
            self.map_fn.return_type(),
        );
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.map_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(&self.map_fn);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.map_fn);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.input_layout.map_layouts(map);
        self.output_layout.map_layouts(map);
        self.map_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.input_layout.remap_layouts(mappings);
        self.output_layout.remap_layouts(mappings);
        self.map_fn.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Map
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("map")
            .append(alloc.space())
            .append(self.input_layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("map_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.map_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Filter {
    input: NodeId,
    filter_fn: Function,
}

impl Filter {
    pub fn new(input: NodeId, filter_fn: Function) -> Self {
        Self { input, filter_fn }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn filter_fn(&self) -> &Function {
        &self.filter_fn
    }
}

impl DataflowNode for Filter {
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
        inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(match inputs[0] {
            StreamLayout::Set(value) => StreamLayout::Set(value),
            StreamLayout::Map(key, value) => StreamLayout::Map(key, value),
        })
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        // TODO
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.filter_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(&self.filter_fn);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.filter_fn);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.filter_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.filter_fn.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Filter
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("filter")
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("filter_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.filter_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FilterMap {
    input: NodeId,
    filter_map: Function,
    layout: LayoutId,
}

impl FilterMap {
    pub fn new(input: NodeId, filter_map: Function, layout: LayoutId) -> Self {
        Self {
            input,
            filter_map,
            layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn filter_map(&self) -> &Function {
        &self.filter_map
    }

    pub const fn layout(&self) -> LayoutId {
        self.layout
    }
}

impl DataflowNode for FilterMap {
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
        inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        Some(match inputs[0] {
            StreamLayout::Set(_) => StreamLayout::Set(self.layout),
            StreamLayout::Map(key_layout, _) => StreamLayout::Map(key_layout, self.layout),
        })
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {
        // TODO
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.filter_map.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(self.filter_map());
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.filter_map);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.layout);
        self.filter_map.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.layout = mappings[&self.layout];
        self.filter_map.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &FilterMap
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("filter_map")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("filter_map:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.filter_map.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}
