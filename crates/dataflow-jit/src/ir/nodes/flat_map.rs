// Basic sketch:
// - Flat map fn gets its normal params plus four additional pointers, `keys:
//   &mut Vec<Row>`, `values: &mut Vec<Row>` and the vtable pointers for the key
//   and value rows
// - User calls `@dbsp.row.vec.push(vec: &mut Vec<Row>, row_ptr: *mut RawRow)`
//   to push an element to a vec
// - User produces outputs with successive `@dbsp.row.vec.push()` calls to key
//   and value vecs
// - `@dbsp.row.vec.push(vec: &mut Vec<Row>, row_ptr: *mut RawRow)`, takes the
//   raw data of a row, allocates a `Row`, copies the raw data into it and sets
//   the proper vtable pointer before pushing the row value to the vec

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

/// The flat map operator produces multiple (zero or more) outputs for each
/// input pair
///
/// It's fairly polymorphic in its design, allowing taking set and map inputs
/// and producing set and map outputs. A set or map can be produced regardless
/// of the input stream's type.
///
/// If the input stream is a set, one key param will be required of the
/// `flat_map` function. If the input stream is a map, one key param and one
/// value param will be required. If the output stream is a set, one param with
/// a layout of `{ ptr, ptr }` will be required, this is the vector that
/// produced keys can be pushed to using `@dbsp.row.vec.push()`. If the output
/// stream is a map, two params with layouts of `{ ptr, ptr }` will be required,
/// forming the key and values vectors that produced keys and values can be
/// pushed to using `@dbsp.row.vec.push()`.
///
/// #### Possible Function Signatures
///
/// | Input Stream | Output Stream | Function Signature
/// | | :----------: | :-----------: |
/// ------------------------------------------------------------------------------------------
/// | |     Set      |      Set      | `fn(key: { ... }, output_keys: { ptr, ptr
/// })`                                              | |     Set      |      Map
/// | `fn(key: { ... }, output_keys: { ptr, ptr }, output_values: { ptr, ptr })`
/// | |     Map      |      Set      | `fn(key: { ... }, value: { ... },
/// output_keys: { ptr, ptr })`                              | |     Map      |
/// Map      | `fn(key: { ... }, value: { ... }, output_keys: { ptr, ptr },
/// output_values: { ptr, ptr })` |
///
/// `{ ... }` denotes an arbitrary layout, the `output_keys` and `output_values`
/// vectors are typed in association with `output_layout`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FlatMap {
    input: NodeId,
    flat_map: Function,
    output_layout: StreamLayout,
    // TODO: Allow the compiler to infer an output size hint
    //       to enable buffer pre-allocation
}

impl FlatMap {
    pub fn new(input: NodeId, flat_map: Function, output_layout: StreamLayout) -> Self {
        Self {
            input,
            flat_map,
            output_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn flat_map(&self) -> &Function {
        &self.flat_map
    }

    pub const fn output_layout(&self) -> StreamLayout {
        self.output_layout
    }
}

impl DataflowNode for FlatMap {
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

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.flat_map.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.push(&self.flat_map);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.push(&mut self.flat_map);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.output_layout.map_layouts(map);
        self.flat_map.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.output_layout.remap_layouts(mappings);
        self.flat_map.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &FlatMap
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("flat_map")
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.output_layout.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("flat_map:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.flat_map.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}
