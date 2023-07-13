use crate::ir::{
    function::Function,
    layout_cache::RowLayoutCache,
    literal::RowLiteral,
    nodes::{DataflowNode, StreamLayout},
    pretty::{DocAllocator, DocBuilder, Pretty},
    ColumnType, InputFlags, LayoutId, NodeId, RowLayoutBuilder,
};
use dbsp::operator::time_series::RelRange;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Min {
    input: NodeId,
    layout: StreamLayout,
}

impl Min {
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

impl DataflowNode for Min {
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

impl<'a, D, A> Pretty<'a, D, A> for &Min
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("min")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Max {
    input: NodeId,
    layout: StreamLayout,
    // TODO: Should we allow the output layout to be different?
}

impl Max {
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

impl DataflowNode for Max {
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

impl<'a, D, A> Pretty<'a, D, A> for &Max
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("max")
            .append(alloc.space())
            .append(self.layout.pretty(alloc, cache))
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Fold {
    input: NodeId,
    /// The initial value of the fold, should be the same layout as `acc_layout`
    init: RowLiteral,
    /// The step function, should have a signature of
    /// `fn(acc_layout, step_layout, weight_layout) -> acc_layout`
    step_fn: Function,
    /// The finish function, should have a signature of
    /// `fn(acc_layout) -> output_layout`
    finish_fn: Function,
    /// The layout of the accumulator value
    acc_layout: LayoutId,
    /// The layout of the step value
    step_layout: LayoutId,
    /// The layout of the output stream
    output_layout: LayoutId,
}

impl Fold {
    pub fn new(
        input: NodeId,
        init: RowLiteral,
        step_fn: Function,
        finish_fn: Function,
        acc_layout: LayoutId,
        step_layout: LayoutId,
        output_layout: LayoutId,
    ) -> Self {
        Self {
            input,
            init,
            step_fn,
            finish_fn,
            acc_layout,
            step_layout,
            output_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn init(&self) -> &RowLiteral {
        &self.init
    }

    pub const fn step_fn(&self) -> &Function {
        &self.step_fn
    }

    pub const fn finish_fn(&self) -> &Function {
        &self.finish_fn
    }

    pub const fn acc_layout(&self) -> LayoutId {
        self.acc_layout
    }

    pub const fn step_layout(&self) -> LayoutId {
        self.step_layout
    }

    pub const fn output_layout(&self) -> LayoutId {
        self.output_layout
    }
}

impl DataflowNode for Fold {
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
        // FIXME: Should this be able to operate on sets too?
        Some(StreamLayout::Map(
            inputs[0].unwrap_map().0,
            self.output_layout,
        ))
    }

    fn validate(&self, inputs: &[StreamLayout], layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].is_map());
        assert_eq!(inputs[0].unwrap_map().1, self.step_layout);

        // Step function
        {
            assert_eq!(self.step_fn.args().len(), 3);

            let acc_arg = &self.step_fn.args()[0];
            assert_eq!(acc_arg.layout, self.acc_layout);
            assert_eq!(acc_arg.flags, InputFlags::INOUT);

            let step_arg = &self.step_fn.args()[1];
            assert_eq!(step_arg.layout, self.step_layout);
            assert_eq!(step_arg.flags, InputFlags::INPUT);

            let weight_layout = layout_cache.add(
                RowLayoutBuilder::new()
                    .with_column(ColumnType::I32, false)
                    .build(),
            );
            let weight_arg = &self.step_fn.args()[2];
            assert_eq!(weight_arg.layout, weight_layout);
            assert_eq!(weight_arg.flags, InputFlags::INPUT);
        }

        // Finish function
        {
            assert_eq!(self.finish_fn.args().len(), 2);

            let acc_arg = &self.finish_fn.args()[0];
            assert_eq!(acc_arg.layout, self.acc_layout);
            assert_eq!(acc_arg.flags, InputFlags::INPUT);

            let output_arg = &self.finish_fn.args()[1];
            assert_eq!(output_arg.layout, self.output_layout);
            assert_eq!(output_arg.flags, InputFlags::OUTPUT);
        }
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.step_fn.optimize(layout_cache);
        self.finish_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.extend([self.step_fn(), self.finish_fn()]);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.extend([&mut self.step_fn, &mut self.finish_fn]);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.acc_layout);
        map(self.step_layout);
        map(self.output_layout);
        self.step_fn.map_layouts(&mut *map);
        self.finish_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.acc_layout = mappings[&self.acc_layout];
        self.step_layout = mappings[&self.step_layout];
        self.output_layout = mappings[&self.output_layout];
        self.step_fn.remap_layouts(mappings);
        self.finish_fn.remap_layouts(mappings);
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &Fold
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("fold")
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.output_layout.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("step_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.step_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("finish_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.finish_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PartitionedRollingFold {
    input: NodeId,
    /// The time range to fold over
    // FIXME: Support more timestamps
    range: RelRange<i64>,
    /// The initial value of the fold, should be the same layout as `acc_layout`
    init: RowLiteral,
    /// The step function, should have a signature of
    /// `fn(acc_layout, step_layout, weight_layout) -> acc_layout`
    step_fn: Function,
    /// The finish function, should have a signature of
    /// `fn(acc_layout) -> output_layout`
    finish_fn: Function,
    /// The layout of the accumulator value
    acc_layout: LayoutId,
    /// The layout of the step value
    step_layout: LayoutId,
    /// The layout of the output stream
    output_layout: LayoutId,
}

impl PartitionedRollingFold {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input: NodeId,
        range: RelRange<i64>,
        init: RowLiteral,
        step_fn: Function,
        finish_fn: Function,
        acc_layout: LayoutId,
        step_layout: LayoutId,
        output_layout: LayoutId,
    ) -> Self {
        Self {
            input,
            range,
            init,
            step_fn,
            finish_fn,
            acc_layout,
            step_layout,
            output_layout,
        }
    }

    pub const fn input(&self) -> NodeId {
        self.input
    }

    pub const fn init(&self) -> &RowLiteral {
        &self.init
    }

    pub const fn step_fn(&self) -> &Function {
        &self.step_fn
    }

    pub const fn finish_fn(&self) -> &Function {
        &self.finish_fn
    }

    pub const fn acc_layout(&self) -> LayoutId {
        self.acc_layout
    }

    pub const fn step_layout(&self) -> LayoutId {
        self.step_layout
    }

    pub const fn output_layout(&self) -> LayoutId {
        self.output_layout
    }

    pub const fn range(&self) -> RelRange<i64> {
        self.range
    }
}

impl DataflowNode for PartitionedRollingFold {
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
        // FIXME: Should this be able to operate on sets too?
        Some(StreamLayout::Map(
            inputs[0].unwrap_map().0,
            self.output_layout,
        ))
    }

    fn validate(&self, inputs: &[StreamLayout], layout_cache: &RowLayoutCache) {
        assert_eq!(inputs.len(), 1);
        assert!(inputs[0].is_map());
        assert_eq!(inputs[0].unwrap_map().1, self.step_layout);

        // Step function
        {
            assert_eq!(self.step_fn.args().len(), 3);

            let acc_arg = &self.step_fn.args()[0];
            assert_eq!(acc_arg.layout, self.acc_layout);
            assert_eq!(acc_arg.flags, InputFlags::INOUT);

            let step_arg = &self.step_fn.args()[1];
            assert_eq!(step_arg.layout, self.step_layout);
            assert_eq!(step_arg.flags, InputFlags::INPUT);

            let weight_layout = layout_cache.add(
                RowLayoutBuilder::new()
                    .with_column(ColumnType::I32, false)
                    .build(),
            );
            let weight_arg = &self.step_fn.args()[2];
            assert_eq!(weight_arg.layout, weight_layout);
            assert_eq!(weight_arg.flags, InputFlags::INPUT);
        }

        // Finish function
        {
            assert_eq!(self.finish_fn.args().len(), 2);

            let acc_arg = &self.finish_fn.args()[0];
            assert_eq!(acc_arg.layout, self.acc_layout);
            assert_eq!(acc_arg.flags, InputFlags::INPUT);

            let output_arg = &self.finish_fn.args()[1];
            assert_eq!(output_arg.layout, self.output_layout);
            assert_eq!(output_arg.flags, InputFlags::OUTPUT);
        }
    }

    fn optimize(&mut self, layout_cache: &RowLayoutCache) {
        self.step_fn.optimize(layout_cache);
        self.finish_fn.optimize(layout_cache);
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        functions.extend([self.step_fn(), self.finish_fn()]);
    }

    fn functions_mut<'a>(&'a mut self, functions: &mut Vec<&'a mut Function>) {
        functions.extend([&mut self.step_fn, &mut self.finish_fn]);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        map(self.acc_layout);
        map(self.step_layout);
        map(self.output_layout);
        self.step_fn.map_layouts(&mut *map);
        self.finish_fn.map_layouts(map);
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.acc_layout = mappings[&self.acc_layout];
        self.step_layout = mappings[&self.step_layout];
        self.output_layout = mappings[&self.output_layout];
        self.step_fn.remap_layouts(mappings);
        self.finish_fn.remap_layouts(mappings);
    }
}

impl JsonSchema for PartitionedRollingFold {
    fn schema_name() -> String {
        "PartitionedRollingFold".to_owned()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        let mut schema_object = schemars::schema::SchemaObject {
            instance_type: Some(schemars::schema::InstanceType::Object.into()),
            ..Default::default()
        };

        let object_validation = schema_object.object();

        object_validation
            .properties
            .insert("input".to_owned(), gen.subschema_for::<NodeId>());
        object_validation.required.insert("input".to_owned());

        #[derive(JsonSchema)]
        #[allow(dead_code)]
        enum RelOffset<TS> {
            Before(TS),
            After(TS),
        }

        #[derive(JsonSchema)]
        #[allow(dead_code)]
        struct RelRange<TS> {
            from: RelOffset<TS>,
            to: RelOffset<TS>,
        }

        object_validation.properties.insert(
            "range".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for::<RelRange<i64>>(),
                schemars::schema::Metadata {
                    description: Some("The time range to fold over".to_owned()),
                    ..Default::default()
                },
            ),
        );
        object_validation.required.insert("range".to_owned());

        object_validation.properties.insert(
            "init".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for::<RowLiteral>(),
                schemars::schema::Metadata {
                    description: Some(
                        "The initial value of the fold, should be the same layout as `acc_layout`"
                            .to_owned(),
                    ),
                    ..Default::default()
                },
            ),
        );
        object_validation.required.insert("init".to_owned());

        object_validation.properties.insert(
            "step_fn".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for:: <Function>(),
                schemars::schema::Metadata {
                    description: Some(
                        "The step function, should have a signature of `fn(acc_layout, step_layout, weight_layout) -> acc_layout`".to_owned(),
                    ),
                    ..Default::default()
                },
            ),
        );
        object_validation.required.insert("step_fn".to_owned());

        object_validation.properties.insert(
            "finish_fn".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for:: <Function>(),
                schemars::schema::Metadata {
                    description: Some(
                        "The finish function, should have a signature of `fn(acc_layout) -> output_layout`".to_owned(),
                    ),
                    ..Default::default()
                },
            ),
        );
        object_validation.required.insert("finish_fn".to_owned());

        object_validation.properties.insert(
            "acc_layout".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for::<LayoutId>(),
                schemars::schema::Metadata {
                    description: Some("The layout of the accumulator value".to_owned()),
                    ..Default::default()
                },
            ),
        );
        object_validation.required.insert("acc_layout".to_owned());

        object_validation.properties.insert(
            "step_layout".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for::<LayoutId>(),
                schemars::schema::Metadata {
                    description: Some("The layout of the step value".to_owned()),
                    ..Default::default()
                },
            ),
        );
        object_validation.required.insert("step_layout".to_owned());

        object_validation.properties.insert(
            "output_layout".to_owned(),
            schemars::_private::apply_metadata(
                gen.subschema_for::<LayoutId>(),
                schemars::schema::Metadata {
                    description: Some("The layout of the output stream".to_owned()),
                    ..Default::default()
                },
            ),
        );
        object_validation
            .required
            .insert("output_layout".to_owned());

        schemars::schema::Schema::Object(schema_object)
    }
}

impl<'a, D, A> Pretty<'a, D, A> for &PartitionedRollingFold
where
    A: 'a,
    D: DocAllocator<'a, A> + ?Sized + 'a,
    DocBuilder<'a, D, A>: Clone,
{
    fn pretty(self, alloc: &'a D, cache: &RowLayoutCache) -> DocBuilder<'a, D, A> {
        alloc
            .text("partitioned_rolling_fold")
            .append(alloc.space())
            .append(self.input.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(self.output_layout.pretty(alloc, cache))
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("step_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.step_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
            .append(alloc.text(","))
            .append(alloc.space())
            .append(alloc.text("finish_fn:"))
            .append(alloc.space())
            .append(
                alloc
                    .hardline()
                    .append(self.finish_fn.pretty(alloc, cache).indent(2))
                    .append(alloc.hardline())
                    .braces(),
            )
    }
}
