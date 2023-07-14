use crate::{
    codegen::TRIG_INTRINSICS,
    ir::{
        exprs::RowOrScalar,
        exprs::{
            BinaryOp, BinaryOpKind, Cast, Constant, Drop, Expr, ExprId, IsNull, Load, NullRow,
            RValue, SetNull, Store, UnaryOpKind, Uninit, UninitRow,
        },
        exprs::{Call, Select},
        graph::GraphExt,
        nodes::{
            Antijoin, ConstantStream, DataflowNode, DelayedFeedback, Delta0, Differentiate,
            Distinct, Export, ExportedNode, Filter, FilterMap, FlatMap, Fold, IndexByColumn,
            IndexWith, Integrate, Map, Max, Min, Minus, MonotonicJoin, Neg, Node, NodeId,
            PartitionedRollingFold, Sink, Source, SourceMap, StreamKind, StreamLayout, Subgraph,
            Sum, UnitMapToSet,
        },
        visit::NodeVisitor,
        BlockId, ColumnType, Function, Graph, InputFlags, LayoutId, RowLayoutBuilder,
        RowLayoutCache,
    },
};
use derive_more::Display;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
};

// TODO: Validate block parameters

pub type ValidationResult<T = ()> = Result<T, ValidationError>;

pub struct Validator {
    /// A set of all nodes that exist
    nodes: BTreeSet<NodeId>,
    /// A map of nodes to their inputs (if they accept inputs)
    // TODO: TinyVec<[LayoutId; 5]>
    node_inputs: BTreeMap<NodeId, Vec<NodeId>>,
    /// A map of nodes to their output layout (if they produce an output)
    node_outputs: BTreeMap<NodeId, StreamLayout>,
    function_validator: FunctionValidator,
}

impl Validator {
    pub fn new(layout_cache: RowLayoutCache) -> Self {
        Self {
            nodes: BTreeSet::new(),
            node_inputs: BTreeMap::new(),
            node_outputs: BTreeMap::new(),
            function_validator: FunctionValidator::new(layout_cache),
        }
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.node_inputs.clear();
        self.node_outputs.clear();
    }

    pub const fn layout_cache(&self) -> &RowLayoutCache {
        &self.function_validator.layout_cache
    }

    // FIXME: Make this return a result instead of panicking
    // TODO: Ensure that delta0 only occurs within subgraphs
    // TODO: Validate nested subgraphs
    pub fn validate_graph(&mut self, graph: &Graph) -> ValidationResult {
        self.clear();

        {
            // Collect all nodes and the layouts of their outputs
            let layout_cache = self.layout_cache().clone();
            let mut meta_collector = MetaCollector::new(
                &mut self.nodes,
                &mut self.node_inputs,
                &mut self.node_outputs,
                layout_cache,
            );
            graph.accept(&mut meta_collector);

            if !meta_collector.errors.is_empty() {
                // TODO: Return multiple errors
                return Err(meta_collector.errors.remove(0));
            }
        }

        for (&node_id, node) in graph.nodes() {
            match node {
                Node::Map(map) => {
                    assert_eq!(map.map_fn().return_type(), ColumnType::Unit);

                    let input_layout = self.get_expected_input(node_id, map.input());
                    map.validate(&[input_layout], &self.function_validator.layout_cache);
                    self.function_validator.validate_function(map.map_fn())?;
                }

                Node::Filter(filter) => {
                    let _input_layout = self.get_expected_input(node_id, filter.input());
                    assert_eq!(filter.filter_fn().return_type(), ColumnType::Bool);

                    // TODO: Validate function arguments

                    self.function_validator
                        .validate_function(filter.filter_fn())?;
                }

                Node::Neg(neg) => {
                    let input_layout = self.get_expected_input(node_id, neg.input());
                    assert_eq!(input_layout, neg.layout());
                }

                Node::IndexWith(index_with) => {
                    let _input_layout = self.get_expected_input(node_id, index_with.input());
                    assert_eq!(index_with.index_fn().return_type(), ColumnType::Unit);

                    // TODO: Validate function arguments

                    self.function_validator
                        .validate_function(index_with.index_fn())?;
                }

                Node::IndexByColumn(index_by) => {
                    let input_layout = self.get_expected_input(node_id, index_by.input());
                    assert_eq!(input_layout, StreamLayout::Set(index_by.input_layout()));

                    let (key_layout, value_layout) = {
                        let input_layout = self.layout_cache().get(index_by.input_layout());
                        assert!(
                            index_by.key_column() < input_layout.len(),
                            "key column {} doesn't exist, {input_layout} only has {} columns",
                            index_by.key_column(),
                            input_layout.len(),
                        );
                        for &value in index_by.discarded_values() {
                            assert!(
                                value < input_layout.len(),
                                "discarded value column {value} doesn't exist, {input_layout} only has {} columns",
                                input_layout.len(),
                            );
                        }

                        let key_layout = RowLayoutBuilder::new()
                            .with_column(
                                input_layout.column_type(index_by.key_column()),
                                input_layout.column_nullable(index_by.key_column()),
                            )
                            .build();

                        let mut value_layout = RowLayoutBuilder::new();
                        for (column, (ty, nullable)) in input_layout.iter().enumerate() {
                            if column != index_by.key_column()
                                && !index_by.discarded_values().contains(&column)
                            {
                                value_layout.add_column(ty, nullable);
                            }
                        }

                        (key_layout, value_layout.build())
                    };

                    let expected_key = self.layout_cache().add(key_layout);
                    assert_eq!(index_by.key_layout(), expected_key);

                    let expected_value = self.layout_cache().add(value_layout);
                    assert_eq!(index_by.value_layout(), expected_value);
                }

                Node::JoinCore(join) => {
                    let _lhs_layout = self.get_expected_input(node_id, join.lhs());
                    let _rhs_layout = self.get_expected_input(node_id, join.rhs());
                    assert_eq!(join.join_fn().return_type(), ColumnType::Unit);

                    // TODO: Validate function arguments
                    self.function_validator.validate_function(join.join_fn())?;
                }

                Node::MonotonicJoin(join) => {
                    let _lhs_layout = self.get_expected_input(node_id, join.lhs());
                    let _rhs_layout = self.get_expected_input(node_id, join.rhs());
                    assert_eq!(join.join_fn().return_type(), ColumnType::Unit);

                    // TODO: Validate function arguments
                    self.function_validator.validate_function(join.join_fn())?;
                }

                Node::Fold(fold) => {
                    self.function_validator.validate_function(fold.step_fn())?;
                    self.function_validator
                        .validate_function(fold.finish_fn())?;
                }

                _ => {}
            }
        }

        Ok(())
    }

    #[track_caller]
    fn get_expected_input(&self, node: NodeId, input: NodeId) -> StreamLayout {
        if let Some(&input_layout) = self.node_outputs.get(&input) {
            input_layout
        } else {
            panic!("node {node}'s input {input} does not exist");
        }
    }
}

struct MetaCollector<'a> {
    /// A set of all nodes that exist
    nodes: &'a mut BTreeSet<NodeId>,
    /// A map of nodes to their inputs (if they accept inputs)
    // TODO: TinyVec<[LayoutId; 5]>
    node_inputs: &'a mut BTreeMap<NodeId, Vec<NodeId>>,
    /// A map of nodes to their output layout (if they produce an output)
    node_outputs: &'a mut BTreeMap<NodeId, StreamLayout>,
    layout_cache: RowLayoutCache,
    errors: Vec<ValidationError>,
}

impl<'a> MetaCollector<'a> {
    fn new(
        nodes: &'a mut BTreeSet<NodeId>,
        node_inputs: &'a mut BTreeMap<NodeId, Vec<NodeId>>,
        node_outputs: &'a mut BTreeMap<NodeId, StreamLayout>,
        layout_cache: RowLayoutCache,
    ) -> Self {
        Self {
            nodes,
            node_inputs,
            node_outputs,
            layout_cache,
            errors: Vec::new(),
        }
    }

    fn add_node(&mut self, node_id: NodeId) {
        // If the graph already contained a node with this id
        if !self.nodes.insert(node_id) {
            self.errors.push(ValidationError::DuplicateNode { node_id });
        }
    }

    fn add_unary(&mut self, node_id: NodeId, input: NodeId, output_layout: StreamLayout) {
        self.add_node(node_id);
        self.node_inputs.insert(node_id, vec![input]);
        self.node_outputs.insert(node_id, output_layout);
    }

    fn add_binary(
        &mut self,
        node_id: NodeId,
        lhs: NodeId,
        rhs: NodeId,
        output_layout: StreamLayout,
    ) {
        self.add_node(node_id);
        self.node_inputs.insert(node_id, vec![lhs, rhs]);
        self.node_outputs.insert(node_id, output_layout);
    }
}

impl NodeVisitor for MetaCollector<'_> {
    fn visit_map(&mut self, node_id: NodeId, map: &Map) {
        self.add_unary(node_id, map.input(), map.output_layout());
    }

    fn visit_min(&mut self, node_id: NodeId, min: &Min) {
        self.add_unary(node_id, min.input(), min.layout());
    }

    fn visit_max(&mut self, node_id: NodeId, max: &Max) {
        self.add_unary(node_id, max.input(), max.layout());
    }

    fn visit_neg(&mut self, node_id: NodeId, neg: &Neg) {
        self.add_unary(node_id, neg.input(), neg.layout());
    }

    fn visit_sum(&mut self, node_id: NodeId, sum: &Sum) {
        self.add_node(node_id);
        self.node_inputs.insert(node_id, sum.inputs().to_vec());
        self.node_outputs.insert(node_id, sum.layout());
    }

    fn visit_fold(&mut self, node_id: NodeId, fold: &Fold) {
        let output = StreamLayout::Map(
            self.node_outputs[&fold.input()].unwrap_map().0,
            fold.output_layout(),
        );
        self.add_unary(node_id, fold.input(), output);

        // FIXME: Move to `DataflowNode::validation()` impl
        if let Err(error) = fold
            .init()
            .validate_layout(node_id, &self.layout_cache.get(fold.acc_layout()))
        {
            self.errors.push(error);
        }
    }

    fn visit_partitioned_rolling_fold(
        &mut self,
        node_id: NodeId,
        rolling_fold: &PartitionedRollingFold,
    ) {
        let output = StreamLayout::Map(
            self.node_outputs[&rolling_fold.input()].unwrap_map().0,
            rolling_fold.output_layout(),
        );
        self.add_unary(node_id, rolling_fold.input(), output);

        // FIXME: Move to `DataflowNode::validation()` impl
        if let Err(error) = rolling_fold
            .init()
            .validate_layout(node_id, &self.layout_cache.get(rolling_fold.acc_layout()))
        {
            self.errors.push(error);
        }
    }

    fn visit_sink(&mut self, node_id: NodeId, sink: &Sink) {
        self.node_inputs.insert(node_id, vec![sink.input()]);
    }

    fn visit_minus(&mut self, node_id: NodeId, minus: &Minus) {
        self.add_binary(
            node_id,
            minus.lhs(),
            minus.rhs(),
            self.node_outputs[&minus.lhs()],
        );
    }

    fn visit_filter(&mut self, node_id: NodeId, filter: &Filter) {
        self.add_unary(node_id, filter.input(), self.node_outputs[&filter.input()]);
    }

    fn visit_filter_map(&mut self, node_id: NodeId, filter_map: &FilterMap) {
        self.add_unary(
            node_id,
            filter_map.input(),
            self.node_outputs[&filter_map.input()],
        );
    }

    fn visit_source(&mut self, node_id: NodeId, source: &Source) {
        self.add_node(node_id);
        self.node_outputs
            .insert(node_id, StreamLayout::Set(source.layout()));
    }

    fn visit_source_map(&mut self, node_id: NodeId, source_map: &SourceMap) {
        self.add_node(node_id);
        self.node_outputs
            .insert(node_id, source_map.output_layout());
    }

    fn visit_index_with(&mut self, node_id: NodeId, index_with: &IndexWith) {
        self.add_unary(node_id, index_with.input(), index_with.output_layout())
    }

    fn visit_differentiate(&mut self, node_id: NodeId, differentiate: &Differentiate) {
        self.add_unary(node_id, differentiate.input(), differentiate.layout());
    }

    fn visit_integrate(&mut self, node_id: NodeId, integrate: &Integrate) {
        self.add_unary(node_id, integrate.input(), integrate.layout());
    }

    fn visit_delta0(&mut self, node_id: NodeId, delta0: &Delta0) {
        self.add_unary(node_id, delta0.input(), self.node_outputs[&delta0.input()]);
    }

    fn visit_delayed_feedback(&mut self, node_id: NodeId, delayed_feedback: &DelayedFeedback) {
        self.add_node(node_id);
        self.node_outputs
            .insert(node_id, StreamLayout::Set(delayed_feedback.layout()));
        // TODO: Delayed feedback inputs?
    }

    fn visit_distinct(&mut self, node_id: NodeId, distinct: &Distinct) {
        self.add_unary(node_id, distinct.input(), distinct.layout());
    }

    fn visit_join_core(&mut self, node_id: NodeId, join: &super::nodes::JoinCore) {
        let output = match join.result_kind() {
            StreamKind::Set => {
                if join.value_layout() != self.layout_cache.unit() {
                    self.errors.push(ValidationError::JoinSetValueNotUnit {
                        join: node_id,
                        value_layout: join.value_layout(),
                        layout: self.layout_cache.get(join.value_layout()).to_string(),
                    });
                }

                StreamLayout::Set(join.key_layout())
            }
            StreamKind::Map => StreamLayout::Map(join.key_layout(), join.value_layout()),
        };

        self.add_binary(node_id, join.lhs(), join.rhs(), output);
    }

    fn visit_monotonic_join(&mut self, node_id: NodeId, join: &MonotonicJoin) {
        self.add_binary(
            node_id,
            join.lhs(),
            join.rhs(),
            StreamLayout::Set(join.key_layout()),
        );
    }

    fn visit_antijoin(&mut self, node_id: NodeId, antijoin: &Antijoin) {
        self.add_binary(node_id, antijoin.lhs(), antijoin.rhs(), antijoin.layout());
    }

    fn visit_constant(&mut self, node_id: NodeId, constant: &ConstantStream) {
        self.add_node(node_id);
        // TODO: Verify that the layout of the row matches its literal value
        self.node_outputs.insert(node_id, constant.layout());

        // FIXME: Move to `DataflowNode::validation()` impl
        constant
            .value()
            .validate_layout(node_id, &self.layout_cache, &mut self.errors);
    }

    fn visit_flat_map(&mut self, node_id: NodeId, flat_map: &FlatMap) {
        self.add_unary(node_id, flat_map.input(), flat_map.output_layout());
    }

    fn visit_index_by_column(&mut self, node_id: NodeId, index_by: &IndexByColumn) {
        self.add_unary(
            node_id,
            index_by.input(),
            StreamLayout::Map(index_by.key_layout(), index_by.value_layout()),
        );
    }

    fn visit_unit_map_to_set(&mut self, node_id: NodeId, map_to_set: &UnitMapToSet) {
        self.add_unary(
            node_id,
            map_to_set.input(),
            StreamLayout::Set(map_to_set.value_layout()),
        );
    }

    fn visit_subgraph(&mut self, node_id: NodeId, subgraph: &Subgraph) {
        self.add_node(node_id);

        // Only validate subgraphs when all above graphs are valid
        if self.errors.is_empty() {
            self.enter_subgraph(node_id, subgraph);
            for (&node_id, node) in subgraph.nodes() {
                node.accept(node_id, self);
            }
            self.leave_subgraph(node_id, subgraph);
        }
    }

    // TODO: Do exports need any special handling?
    fn visit_export(&mut self, node_id: NodeId, export: &Export) {
        self.add_unary(node_id, export.input(), export.layout());
    }

    fn visit_exported_node(&mut self, node_id: NodeId, exported_node: &ExportedNode) {
        self.add_unary(node_id, exported_node.input(), exported_node.layout());
    }
}

pub struct FunctionValidator {
    exprs: BTreeSet<ExprId>,
    /// Expressions that produce values will have a type which will
    /// either be a row type or an entire row
    expr_types: BTreeMap<ExprId, Result<ColumnType, LayoutId>>,
    /// A map from all expressions containing row types to their mutability
    expr_row_mutability: BTreeMap<ExprId, bool>,
    blocks: BTreeSet<BlockId>,
    // TODO: Block parameters once those are implemented
    // TODO: Control flow validation
    layout_cache: RowLayoutCache,
}

impl FunctionValidator {
    pub fn new(layout_cache: RowLayoutCache) -> Self {
        Self {
            exprs: BTreeSet::new(),
            expr_types: BTreeMap::new(),
            expr_row_mutability: BTreeMap::new(),
            blocks: BTreeSet::new(),
            layout_cache,
        }
    }

    pub fn clear(&mut self) {
        self.exprs.clear();
        self.expr_types.clear();
        self.expr_row_mutability.clear();
        self.blocks.clear();
    }

    pub fn validate_function(&mut self, func: &Function) -> ValidationResult {
        self.clear();

        self.blocks.extend(func.blocks().keys().copied());

        // Find any duplicated expressions
        for block in func.blocks().values() {
            for &(expr_id, _) in block.body() {
                if !self.exprs.insert(expr_id) {
                    return Err(ValidationError::DuplicateExpr { expr: expr_id });
                }
            }
        }

        for arg in func.args() {
            if !self.exprs.insert(arg.id) {
                return Err(ValidationError::DuplicateExpr { expr: arg.id });
            }

            self.expr_types.insert(arg.id, Err(arg.layout));
            self.expr_row_mutability
                .insert(arg.id, arg.flags.contains(InputFlags::OUTPUT));
        }

        // Infer expression types
        let mut stack = vec![func.entry_block()];
        while let Some(block_id) = stack.pop() {
            let block = func
                .blocks()
                .get(&block_id)
                .ok_or(ValidationError::MissingBlock { block: block_id })?;

            if block.id() != block_id {
                return Err(ValidationError::MismatchedBlockId {
                    block_id,
                    internal_block_id: block.id(),
                });
            }

            assert_eq!(
                block.id(),
                block_id,
                "block has mismatched id: {block_id} != {}",
                block.id(),
            );

            for &(expr_id, ref expr) in block.body() {
                match expr {
                    Expr::Call(call) => self.call(expr_id, call)?,
                    Expr::Cast(cast) => self.cast(expr_id, cast)?,
                    Expr::Constant(constant) => self.constant(expr_id, constant)?,
                    Expr::Select(select) => self.select(expr_id, select)?,
                    Expr::Load(load) => self.load(expr_id, load)?,
                    Expr::Store(store) => self.store(expr_id, store)?,
                    Expr::IsNull(is_null) => self.is_null(expr_id, is_null)?,
                    Expr::SetNull(set_null) => self.set_null(expr_id, set_null)?,
                    Expr::NullRow(null_row) => self.null_row(expr_id, null_row)?,
                    Expr::UninitRow(uninit_row) => self.uninit_row(expr_id, uninit_row)?,
                    Expr::Uninit(uninit) => self.uninit(expr_id, uninit)?,
                    Expr::BinOp(binop) => self.binop(expr_id, binop)?,
                    Expr::Drop(drop) => self.drop(expr_id, drop)?,

                    Expr::UnaryOp(unary) => {
                        let value_ty = self.expr_types.get(&unary.value()).unwrap().unwrap();

                        match unary.kind() {
                            UnaryOpKind::Not => {
                                assert!(value_ty.is_int() || value_ty.is_bool());
                                let prev = self.expr_types.insert(expr_id, Ok(value_ty));
                                assert!(prev.is_none());
                            }

                            UnaryOpKind::Neg | UnaryOpKind::Abs => {
                                assert!(value_ty.is_float() || value_ty.is_int());
                                let prev = self.expr_types.insert(expr_id, Ok(value_ty));
                                assert!(prev.is_none());
                            }

                            UnaryOpKind::Ceil
                            | UnaryOpKind::Floor
                            | UnaryOpKind::Trunc
                            | UnaryOpKind::Sqrt => {
                                assert!(value_ty.is_float());
                                let prev = self.expr_types.insert(expr_id, Ok(value_ty));
                                assert!(prev.is_none());
                            }

                            UnaryOpKind::CountOnes
                            | UnaryOpKind::CountZeroes
                            | UnaryOpKind::LeadingOnes
                            | UnaryOpKind::LeadingZeroes
                            | UnaryOpKind::TrailingOnes
                            | UnaryOpKind::TrailingZeroes
                            | UnaryOpKind::BitReverse
                            | UnaryOpKind::ByteReverse => {
                                debug_assert!(value_ty.is_int());
                                let prev = self.expr_types.insert(expr_id, Ok(value_ty));
                                assert!(prev.is_none());
                            }

                            UnaryOpKind::StringLen => {
                                debug_assert!(value_ty.is_string());
                                let prev = self.expr_types.insert(expr_id, Ok(ColumnType::U64));
                                assert!(prev.is_none());
                            }
                        }
                    }

                    Expr::Copy(copy) => {
                        assert_eq!(
                            self.expr_types.get(&copy.value()).unwrap().unwrap(),
                            copy.value_ty(),
                        );
                        let prev = self.expr_types.insert(expr_id, Ok(copy.value_ty()));
                        assert!(prev.is_none());
                    }

                    Expr::CopyRowTo(copy) => {
                        assert_eq!(
                            self.expr_types.get(&copy.src()).unwrap().unwrap_err(),
                            copy.layout(),
                        );
                        assert_eq!(
                            self.expr_types.get(&copy.dest()).unwrap().unwrap_err(),
                            copy.layout(),
                        );
                        assert!(self.expr_row_mutability[&copy.dest()]);

                        let prev = self.expr_types.insert(expr_id, Err(copy.layout()));
                        assert!(prev.is_none());

                        self.expr_row_mutability.insert(copy.dest(), true);
                    }

                    Expr::Nop(_) => {}
                }
            }
        }

        Ok(())
    }

    fn expr_type(&self, expr_id: ExprId) -> ValidationResult<Result<ColumnType, LayoutId>> {
        if let Some(&ty) = self.expr_types.get(&expr_id) {
            Ok(ty)
        } else {
            Err(ValidationError::MissingExpr { expr: expr_id })
        }
    }

    fn add_column_expr(&mut self, expr_id: ExprId, column_type: ColumnType) {
        let prev = self.expr_types.insert(expr_id, Ok(column_type));
        debug_assert!(
            prev.is_none(),
            "all duplicate expressions should be caught earlier on in validation",
        );
    }

    fn cast(&mut self, expr_id: ExprId, cast: &Cast) -> ValidationResult {
        if cast.is_valid_cast() {
            let prev = self.expr_types.insert(expr_id, Ok(cast.to()));
            assert!(prev.is_none());
            Ok(())
        } else {
            Err(ValidationError::InvalidCast {
                expr: expr_id,
                from: cast.from(),
                to: cast.to(),
            })
        }
    }

    fn constant(&mut self, expr_id: ExprId, constant: &Constant) -> ValidationResult {
        self.add_column_expr(expr_id, constant.column_type());
        Ok(())
    }

    fn load(&mut self, expr_id: ExprId, load: &Load) -> ValidationResult {
        let source_layout = if let Err(row_layout) = self.expr_type(load.source())? {
            row_layout
        } else {
            return Err(ValidationError::LoadFromScalar {
                load: expr_id,
                source: load.source(),
            });
        };

        if source_layout != load.source_layout() {
            return Err(ValidationError::MismatchedLoadLayout {
                load: expr_id,
                source: load.source(),
                expected_layout: load.source_layout(),
                actual_layout: source_layout,
            });
        }

        {
            let layout = self.layout_cache.get(source_layout);
            if let Some(source_type) = layout.try_column_type(load.column()) {
                if source_type != load.column_type() {
                    return Err(ValidationError::InvalidLoadType {
                        load: expr_id,
                        load_type: load.column_type(),
                        source: load.source(),
                        source_type,
                    });
                }
            } else {
                return Err(ValidationError::InvalidColumnLoad {
                    load: expr_id,
                    column: load.column(),
                    source: load.source(),
                    source_columns: layout.len(),
                    layout: layout.to_string(),
                });
            }
        }

        self.add_column_expr(expr_id, load.column_type());

        Ok(())
    }

    fn store(&mut self, expr_id: ExprId, store: &Store) -> ValidationResult {
        let target_layout = if let Err(row_layout) = self.expr_type(store.target())? {
            row_layout
        } else {
            return Err(ValidationError::StoreToScalar {
                store: expr_id,
                target: store.target(),
            });
        };

        if target_layout != store.target_layout() {
            return Err(ValidationError::MismatchedStoreLayout {
                store: expr_id,
                target: store.target(),
                expected_layout: store.target_layout(),
                actual_layout: target_layout,
            });
        }

        let value_type = match store.value() {
            &RValue::Expr(value) => {
                self.expr_type(value)?
                    .map_err(|_| ValidationError::StoreWithRow {
                        store: expr_id,
                        value,
                    })?
            }
            RValue::Imm(imm) => imm.column_type(),
        };

        {
            let layout = self.layout_cache.get(target_layout);
            if let Some(target_type) = layout.try_column_type(store.column()) {
                if target_type != store.value_type() {
                    return Err(ValidationError::InvalidStoreType {
                        store: expr_id,
                        store_type: store.value_type(),
                        target: store.target(),
                        target_type,
                    });
                } else if target_type != value_type {
                    todo!("invalid store value type in store {expr_id}, tried to store value of type {value_type} to column of type {target_type}")
                }
            } else {
                return Err(ValidationError::InvalidColumnStore {
                    store: expr_id,
                    column: store.column(),
                    target: store.target(),
                    target_columns: layout.len(),
                    layout: layout.to_string(),
                });
            }
        }

        // Ensure we're only storing to mutable rows
        if !*self.expr_row_mutability.get(&store.target()).unwrap() {
            panic!(
                "store {expr_id} stored to {} which is an immutable row",
                store.target(),
            );
        }

        Ok(())
    }

    fn is_null(&mut self, expr_id: ExprId, is_null: &IsNull) -> ValidationResult {
        let target_layout = if let Err(row_layout) = self.expr_type(is_null.target())? {
            row_layout
        } else {
            todo!("called IsNull on scalar")
        };

        {
            let layout = self.layout_cache.get(target_layout);
            if let Some(nullable) = layout.try_column_nullable(is_null.column()) {
                if !nullable {
                    todo!("called IsNull on non-nullable column")
                }
            } else {
                todo!("IsNull column {} doesn't exist", is_null.column())
            }
        }

        self.add_column_expr(expr_id, ColumnType::Bool);

        Ok(())
    }

    fn set_null(&mut self, expr_id: ExprId, set_null: &SetNull) -> ValidationResult {
        let target_layout = if let Err(row_layout) = self.expr_type(set_null.target())? {
            row_layout
        } else {
            todo!("called SetNull on scalar")
        };

        {
            let layout = self.layout_cache.get(target_layout);
            if let Some(nullable) = layout.try_column_nullable(set_null.column()) {
                if !nullable {
                    todo!("called SetNull on non-nullable column")
                }
            } else {
                todo!("SetNull column {} doesn't exist", set_null.column())
            }
        }

        // Make sure that is_null is a boolean value
        match set_null.is_null() {
            &RValue::Expr(is_null) => {
                let ty = self
                    .expr_type(is_null)?
                    .expect("attempted to use row value with SetNull");
                if !ty.is_bool() {
                    todo!("SetNull with non-bool value")
                }
            }

            RValue::Imm(constant) => {
                if !constant.is_bool() {
                    todo!("SetNull with non-bool constant")
                }
            }
        }

        // Ensure we're only storing to mutable rows
        if !*self.expr_row_mutability.get(&set_null.target()).unwrap() {
            panic!(
                "SetNull {expr_id} stored to {} which is an immutable row",
                set_null.target(),
            );
        }

        Ok(())
    }

    fn null_row(&mut self, expr_id: ExprId, null_row: &NullRow) -> ValidationResult {
        self.expr_row_mutability.insert(expr_id, true);
        self.expr_types.insert(expr_id, Err(null_row.layout()));
        Ok(())
    }

    fn uninit_row(&mut self, expr_id: ExprId, uninit_row: &UninitRow) -> ValidationResult {
        self.expr_row_mutability.insert(expr_id, true);
        self.expr_types.insert(expr_id, Err(uninit_row.layout()));
        Ok(())
    }

    fn uninit(&mut self, expr_id: ExprId, uninit: &Uninit) -> ValidationResult {
        match uninit.value() {
            RowOrScalar::Row(layout) => {
                self.expr_row_mutability.insert(expr_id, true);
                self.expr_types.insert(expr_id, Err(layout));
            }

            RowOrScalar::Scalar(scalar_ty) => self.add_column_expr(expr_id, scalar_ty),
        }

        Ok(())
    }

    fn select(&mut self, expr_id: ExprId, select: &Select) -> ValidationResult {
        let cond_ty = self.expr_type(select.cond())?.unwrap();
        assert_eq!(cond_ty, ColumnType::Bool);

        let lhs_ty = self.expr_type(select.cond())?;
        let rhs_ty = self.expr_type(select.cond())?;
        assert_eq!(lhs_ty, rhs_ty);

        self.expr_types.insert(expr_id, lhs_ty);

        Ok(())
    }

    fn call(&mut self, expr_id: ExprId, call: &Call) -> ValidationResult {
        let actual_arg_types = call
            .args()
            .iter()
            .map(|&arg| {
                Ok(match self.expr_type(arg)? {
                    Ok(scalar) => RowOrScalar::Scalar(scalar),
                    Err(layout) => RowOrScalar::Row(layout),
                })
            })
            .collect::<ValidationResult<Vec<_>>>()?;
        assert_eq!(actual_arg_types, call.arg_types());

        match call.function() {
            "dbsp.error.abort" => {
                if !call.args().is_empty() {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 0,
                        args: call.args().len(),
                    });
                }

                assert_eq!(call.ret_ty(), ColumnType::Unit);
            }

            "dbsp.row.vec.push" => {
                if call.args().len() != 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 2,
                        args: call.args().len(),
                    });
                }

                let vec_layout = self.layout_cache.row_vector();
                if actual_arg_types[0] != RowOrScalar::Row(vec_layout) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be vec layout {vec_layout} but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if actual_arg_types[1].is_scalar() {
                    todo!("passed a scalar as the second argument to `@dbsp.row.vec.push()` in {expr_id} when it should be a row value")
                }

                assert_eq!(call.ret_ty(), ColumnType::Unit);
            }

            "dbsp.str.truncate" => {
                if call.args().len() != 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 2,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if actual_arg_types[1] != RowOrScalar::Scalar(ColumnType::Usize) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a usize but instead got {:?}",
                        actual_arg_types[1],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Unit);
            }

            "dbsp.str.truncate_clone" => {
                if call.args().len() != 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 2,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if actual_arg_types[1] != RowOrScalar::Scalar(ColumnType::Usize) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a usize but instead got {:?}",
                        actual_arg_types[1],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::String);
            }

            "dbsp.str.clear" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Unit);
            }

            "dbsp.str.concat" | "dbsp.str.concat_clone" => {
                if call.args().len() < 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 2,
                        args: call.args().len(),
                    });
                }

                for (idx, arg) in actual_arg_types.iter().enumerate() {
                    if arg != &RowOrScalar::Scalar(ColumnType::String) {
                        todo!(
                            "mismatched argument type in {expr_id}, argument {idx} should be a string but instead got {:?}",
                            arg,
                        );
                    }
                }

                assert_eq!(call.ret_ty(), ColumnType::String);
            }

            "dbsp.str.bit_length" | "dbsp.str.char_length" | "dbsp.str.byte_length" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Usize);
            }

            "dbsp.str.is_nfc"
            | "dbsp.str.is_nfd"
            | "dbsp.str.is_nfkc"
            | "dbsp.str.is_nfkd"
            | "dbsp.str.is_lowercase"
            | "dbsp.str.is_uppercase"
            | "dbsp.str.is_ascii" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Bool);
            }

            "dbsp.str.write" => {
                if call.args().len() != 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 2,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, argument 0 should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if !actual_arg_types[1].is_scalar() {
                    todo!(
                        "mismatched argument type in {expr_id}, argument 1 should be a scalar but instead got {:?}",
                        actual_arg_types[1],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::String);
            }

            "dbsp.timestamp.epoch"
            | "dbsp.timestamp.year"
            | "dbsp.timestamp.month"
            | "dbsp.timestamp.day"
            | "dbsp.timestamp.quarter"
            | "dbsp.timestamp.decade"
            | "dbsp.timestamp.century"
            | "dbsp.timestamp.millennium"
            | "dbsp.timestamp.iso_year"
            | "dbsp.timestamp.week"
            | "dbsp.timestamp.day_of_week"
            | "dbsp.timestamp.iso_day_of_week"
            | "dbsp.timestamp.day_of_year"
            | "dbsp.timestamp.millisecond"
            | "dbsp.timestamp.microsecond"
            | "dbsp.timestamp.second"
            | "dbsp.timestamp.minute"
            | "dbsp.timestamp.hour" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::Timestamp) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a timestamp but instead got {:?}",
                        actual_arg_types[0],
                    );
                }
            }

            "dbsp.timestamp.floor_week" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::Timestamp) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a timestamp but instead got {:?}",
                        actual_arg_types[0],
                    );
                }
            }

            "dbsp.timestamp.to_date" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::Timestamp) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a timestamp but instead got {:?}",
                        actual_arg_types[0],
                    );
                }
            }

            "dbsp.date.hour"
            | "dbsp.date.minute"
            | "dbsp.date.second"
            | "dbsp.date.millisecond"
            | "dbsp.date.microsecond"
            | "dbsp.date.year"
            | "dbsp.date.month"
            | "dbsp.date.day"
            | "dbsp.date.quarter"
            | "dbsp.date.decade"
            | "dbsp.date.century"
            | "dbsp.date.millennium"
            | "dbsp.date.iso_year"
            | "dbsp.date.week"
            | "dbsp.date.day_of_week"
            | "dbsp.date.iso_day_of_week"
            | "dbsp.date.day_of_year"
            | "dbsp.date.epoch" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::Date) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a date but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if call.function() == "dbsp.date.epoch" {
                    assert_eq!(call.ret_ty(), ColumnType::I64);
                }
            }

            "dbsp.date.to_timestamp" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::Date) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a date but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Timestamp);
            }

            "dbsp.math.is_power_of_two" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if !actual_arg_types[0]
                    .as_scalar()
                    .map_or(false, ColumnType::is_unsigned_int)
                {
                    todo!(
                        "mismatched argument type in {expr_id}, should be an unsigned integer but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Bool);
            }

            "dbsp.math.fdim" => {
                if call.args().len() != 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 2,
                        args: call.args().len(),
                    });
                }

                if !actual_arg_types[0]
                    .as_scalar()
                    .map_or(false, ColumnType::is_float)
                {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a float but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if !actual_arg_types[1]
                    .as_scalar()
                    .map_or(false, ColumnType::is_float)
                {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a float but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                let (x_ty, y_ty) = (
                    actual_arg_types[0].as_scalar().unwrap(),
                    actual_arg_types[1].as_scalar().unwrap(),
                );
                if x_ty != y_ty {
                    todo!(
                        "mismatched argument types in {expr_id}, both arguments should be the same type. \
                        First argument has the type {x_ty} and the second arg has the type {y_ty}",
                    )
                }
            }

            trig if TRIG_INTRINSICS.contains(&trig)
                || [
                    "dbsp.math.cot",
                    "dbsp.math.degrees_to_radians",
                    "dbsp.math.radians_to_degrees",
                ]
                .contains(&trig) =>
            {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if !actual_arg_types[0]
                    .as_scalar()
                    .map_or(false, ColumnType::is_float)
                {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a float but instead got {:?}",
                        actual_arg_types[0],
                    );
                }
            }

            "dbsp.str.with.capacity" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::Usize) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a usize but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::String);
            }

            "dbsp.str.parse" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }
            }

            "dbsp.io.str.print" => {
                if call.args().len() != 1 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if actual_arg_types[0] != RowOrScalar::Scalar(ColumnType::String) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a string but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                assert_eq!(call.ret_ty(), ColumnType::Unit);
            }

            "dbsp.sql.round" => {
                if call.args().len() != 2 {
                    return Err(ValidationError::IncorrectFunctionArgLen {
                        expr_id,
                        function: call.function().to_owned(),
                        expected_args: 1,
                        args: call.args().len(),
                    });
                }

                if !actual_arg_types[0]
                    .as_scalar()
                    .map_or(false, ColumnType::is_float)
                {
                    todo!(
                        "mismatched argument type in {expr_id}, should be a float but instead got {:?}",
                        actual_arg_types[0],
                    );
                }

                if actual_arg_types[1] != RowOrScalar::Scalar(ColumnType::I32) {
                    todo!(
                        "mismatched argument type in {expr_id}, should be an i32 but instead got {:?}",
                        actual_arg_types[1],
                    );
                }

                assert_eq!(call.ret_ty(), actual_arg_types[0].unwrap_scalar());
            }

            unknown => {
                return Err(ValidationError::UnknownFunction {
                    expr_id,
                    function: unknown.to_owned(),
                })
            }
        }

        self.add_column_expr(expr_id, call.ret_ty());

        Ok(())
    }

    fn binop(&mut self, expr_id: ExprId, binop: &BinaryOp) -> ValidationResult {
        let lhs_ty = self.expr_type(binop.lhs())?.unwrap();
        let rhs_ty = self.expr_type(binop.rhs())?.unwrap();

        if lhs_ty != rhs_ty {
            return Err(ValidationError::MismatchedBinaryOperands {
                expr_id,
                binop: binop.kind(),
                lhs: binop.lhs(),
                lhs_ty,
                rhs: binop.rhs(),
                rhs_ty,
            });
        }

        match binop.kind() {
            BinaryOpKind::Eq
            | BinaryOpKind::Neq
            | BinaryOpKind::LessThan
            | BinaryOpKind::GreaterThan
            | BinaryOpKind::LessThanOrEqual
            | BinaryOpKind::GreaterThanOrEqual => {
                let prev = self.expr_types.insert(expr_id, Ok(ColumnType::Bool));
                assert!(prev.is_none());
            }

            BinaryOpKind::Add
            | BinaryOpKind::Sub
            | BinaryOpKind::Mul
            | BinaryOpKind::Div
            | BinaryOpKind::And
            | BinaryOpKind::Or
            | BinaryOpKind::Xor
            | BinaryOpKind::Min
            | BinaryOpKind::Max => {
                assert_ne!(lhs_ty, ColumnType::String);
                let prev = self.expr_types.insert(expr_id, Ok(lhs_ty));
                assert!(prev.is_none());
            }

            BinaryOpKind::Mod => {
                assert!(lhs_ty.is_int() || lhs_ty.is_float());
                let prev = self.expr_types.insert(expr_id, Ok(lhs_ty));
                assert!(prev.is_none());
            }

            // TODO: Implement all of these for floats
            BinaryOpKind::Rem | BinaryOpKind::DivFloor | BinaryOpKind::ModFloor => {
                assert!(lhs_ty.is_int());
                let prev = self.expr_types.insert(expr_id, Ok(lhs_ty));
                assert!(prev.is_none());
            }
        }

        Ok(())
    }

    fn drop(&mut self, expr_id: ExprId, drop: &Drop) -> ValidationResult {
        if !self.exprs.contains(&drop.value()) {
            return Err(ValidationError::MissingExpr { expr: drop.value() });
        }

        let ty = self.expr_types[&drop.value()];
        match (ty, drop.value_type()) {
            (Ok(ty), RowOrScalar::Scalar(drop_ty)) => {
                assert_eq!(ty, drop_ty);
                // TODO: Error
            }

            (Err(ty), RowOrScalar::Row(drop_ty)) => {
                assert_eq!(ty, drop_ty);
                // TODO: Error
            }

            _ => todo!(
                "drop {expr_id} got value {} with type {ty:?} but expected {:?}",
                drop.value(),
                drop.value_type(),
            ),
        }

        Ok(())
    }
}

#[derive(Debug, Display)]
pub enum ValidationError {
    #[display(fmt = "declared node {node_id} multiple times")]
    DuplicateNode { node_id: NodeId },

    #[display(fmt = "attempted to use block that doesn't exist: {block}")]
    MissingBlock { block: BlockId },

    #[display(
        fmt = "mismatched block ids, block was listed under {block_id} but has an internal id of {internal_block_id}"
    )]
    MismatchedBlockId {
        block_id: BlockId,
        internal_block_id: BlockId,
    },

    #[display(fmt = "the expression {expr} was declared multiple times")]
    DuplicateExpr { expr: ExprId },

    #[display(fmt = "invalid cast in {expr}: cannot cast from {from} to {to}")]
    InvalidCast {
        expr: ExprId,
        from: ColumnType,
        to: ColumnType,
    },

    #[display(fmt = "attempted to use expression that doesn't exist: {expr}")]
    MissingExpr { expr: ExprId },

    #[display(
        fmt = "attempted to load from a scalar value and not a row value in load {load} from {source}"
    )]
    LoadFromScalar { load: ExprId, source: ExprId },

    #[display(
        fmt = "attempted to store to a scalar value and not a row value in store {store} to {target}"
    )]
    StoreToScalar { store: ExprId, target: ExprId },

    #[display(
        fmt = "attempted to load a value of type {source_type} from {source} when load {load} expected a value of type {load_type}"
    )]
    InvalidLoadType {
        load: ExprId,
        load_type: ColumnType,
        source: ExprId,
        source_type: ColumnType,
    },

    #[display(
        fmt = "attempted to store a value of type {target_type} to {target} when store {store} expected a value of type {store_type}"
    )]
    InvalidStoreType {
        store: ExprId,
        store_type: ColumnType,
        target: ExprId,
        target_type: ColumnType,
    },

    #[display(
        fmt = "load {load} attempted to load from column {column} of {source} when {source} only has {source_columns} (source layout is {layout})"
    )]
    InvalidColumnLoad {
        load: ExprId,
        column: usize,
        source: ExprId,
        source_columns: usize,
        layout: String,
    },

    #[display(
        fmt = "store {store} attempted to store to column {column} of {target} when {target} only has {target_columns} (source layout is {layout})"
    )]
    InvalidColumnStore {
        store: ExprId,
        column: usize,
        target: ExprId,
        target_columns: usize,
        layout: String,
    },

    #[display(
        fmt = "load {load} attempted to load from {source} with a layout of {expected_layout} but {source} has the layout {actual_layout}"
    )]
    MismatchedLoadLayout {
        load: ExprId,
        source: ExprId,
        expected_layout: LayoutId,
        actual_layout: LayoutId,
    },

    #[display(
        fmt = "store {store} attempted to store to {target} with a layout of {expected_layout} but {target} has the layout {actual_layout}"
    )]
    MismatchedStoreLayout {
        store: ExprId,
        target: ExprId,
        expected_layout: LayoutId,
        actual_layout: LayoutId,
    },

    #[display(
        fmt = "store {store} attempted to store the row value {value} (expected a scalar value)"
    )]
    StoreWithRow { store: ExprId, value: ExprId },

    #[display(
        fmt = "join {join} produces a set but its value layout {value_layout} was {layout}, not {{ unit }}"
    )]
    JoinSetValueNotUnit {
        join: NodeId,
        value_layout: LayoutId,
        layout: String,
    },

    #[display(
        fmt = "unknown function call in expression {expr_id}: `@{function}()` does not exist"
    )]
    UnknownFunction { expr_id: ExprId, function: String },

    #[display(
        fmt = "incorrect number of function arguments to `@{function}()` in {expr_id}, expected {expected_args} arguments but got {args}"
    )]
    IncorrectFunctionArgLen {
        expr_id: ExprId,
        function: String,
        expected_args: usize,
        args: usize,
    },

    #[display(
        fmt = "mismatched binary op types in {expr_id}, `{binop:?}(lhs: {lhs}, rhs: {rhs})` is not valid \
        ({lhs} has the type {lhs_ty} and {rhs} has the type {rhs_ty})"
    )]
    MismatchedBinaryOperands {
        expr_id: ExprId,
        binop: BinaryOpKind,
        lhs: ExprId,
        lhs_ty: ColumnType,
        rhs: ExprId,
        rhs_ty: ColumnType,
    },

    #[display(
        fmt = "mismatched inputs to operator {node_id} expected {received} to have the layout {expected_layout:?} \
        but it has the layout {received_layout:?}"
    )]
    MismatchedOperatorInputs {
        node_id: NodeId,
        expected: NodeId,
        expected_layout: StreamLayout,
        received: NodeId,
        received_layout: StreamLayout,
    },
}

impl Error for ValidationError {}
