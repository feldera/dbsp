use crate::ir::{
    nodes::{
        Antijoin, ConstantStream, DelayedFeedback, Delta0, Differentiate, Distinct, Export,
        ExportedNode, Filter, FilterMap, FlatMap, Fold, IndexByColumn, IndexWith, Integrate,
        JoinCore, Map, Max, Min, Minus, MonotonicJoin, Neg, Node, PartitionedRollingFold, Sink,
        Source, SourceMap, Subgraph, Sum, TopK, UnitMapToSet,
    },
    GraphExt, NodeId,
};

// TODO: Derive macro to generate this

pub trait NodeVisitor {
    fn visit_map(&mut self, _node_id: NodeId, _map: &Map) {}
    fn visit_min(&mut self, _node_id: NodeId, _min: &Min) {}
    fn visit_max(&mut self, _node_id: NodeId, _max: &Max) {}
    fn visit_neg(&mut self, _node_id: NodeId, _neg: &Neg) {}
    fn visit_sum(&mut self, _node_id: NodeId, _sum: &Sum) {}
    fn visit_fold(&mut self, _node_id: NodeId, _fold: &Fold) {}
    fn visit_sink(&mut self, _node_id: NodeId, _sink: &Sink) {}
    fn visit_minus(&mut self, _node_id: NodeId, _minus: &Minus) {}
    fn visit_filter(&mut self, _node_id: NodeId, _filter: &Filter) {}
    fn visit_filter_map(&mut self, _node_id: NodeId, _filter_map: &FilterMap) {}
    fn visit_source(&mut self, _node_id: NodeId, _source: &Source) {}
    fn visit_source_map(&mut self, _node_id: NodeId, _source_map: &SourceMap) {}
    fn visit_index_with(&mut self, _node_id: NodeId, _index_with: &IndexWith) {}
    fn visit_differentiate(&mut self, _node_id: NodeId, _differentiate: &Differentiate) {}
    fn visit_integrate(&mut self, _node_id: NodeId, _integrate: &Integrate) {}
    fn visit_delta0(&mut self, _node_id: NodeId, _delta0: &Delta0) {}
    fn visit_delayed_feedback(&mut self, _node_id: NodeId, _delayed_feedback: &DelayedFeedback) {}
    fn visit_distinct(&mut self, _node_id: NodeId, _distinct: &Distinct) {}
    fn visit_join_core(&mut self, _node_id: NodeId, _join_core: &JoinCore) {}
    fn visit_export(&mut self, _node_id: NodeId, _export: &Export) {}
    fn visit_exported_node(&mut self, _node_id: NodeId, _exported_node: &ExportedNode) {}
    fn visit_monotonic_join(&mut self, _node_id: NodeId, _monotonic_join: &MonotonicJoin) {}
    fn visit_constant(&mut self, _node_id: NodeId, _constant: &ConstantStream) {}
    fn visit_partitioned_rolling_fold(
        &mut self,
        _node_id: NodeId,
        _partitioned_rolling_fold: &PartitionedRollingFold,
    ) {
    }
    fn visit_flat_map(&mut self, _node_id: NodeId, _flat_map: &FlatMap) {}
    fn visit_antijoin(&mut self, _node_id: NodeId, _antijoin: &Antijoin) {}
    fn visit_index_by_column(&mut self, _node_id: NodeId, _index_by_column: &IndexByColumn) {}
    fn visit_unit_map_to_set(&mut self, _node_id: NodeId, _unit_map_to_set: &UnitMapToSet) {}
    fn visit_topk(&mut self, _node_id: NodeId, _topk: &TopK) {}

    fn visit_subgraph(&mut self, node_id: NodeId, subgraph: &Subgraph) {
        self.enter_subgraph(node_id, subgraph);
        for (&node_id, node) in subgraph.nodes() {
            node.accept(node_id, self);
        }
        self.leave_subgraph(node_id, subgraph);
    }
    fn enter_subgraph(&mut self, _node_id: NodeId, _subgraph: &Subgraph) {}
    fn leave_subgraph(&mut self, _node_id: NodeId, _subgraph: &Subgraph) {}
}

pub trait MutNodeVisitor {
    fn visit_map(&mut self, _node_id: NodeId, _map: &mut Map) {}
    fn visit_min(&mut self, _node_id: NodeId, _min: &mut Min) {}
    fn visit_max(&mut self, _node_id: NodeId, _max: &mut Max) {}
    fn visit_neg(&mut self, _node_id: NodeId, _neg: &mut Neg) {}
    fn visit_sum(&mut self, _node_id: NodeId, _sum: &mut Sum) {}
    fn visit_fold(&mut self, _node_id: NodeId, _fold: &mut Fold) {}
    fn visit_sink(&mut self, _node_id: NodeId, _sink: &mut Sink) {}
    fn visit_minus(&mut self, _node_id: NodeId, _minus: &mut Minus) {}
    fn visit_filter(&mut self, _node_id: NodeId, _filter: &mut Filter) {}
    fn visit_filter_map(&mut self, _node_id: NodeId, _filter_map: &mut FilterMap) {}
    fn visit_source(&mut self, _node_id: NodeId, _source: &mut Source) {}
    fn visit_source_map(&mut self, _node_id: NodeId, _source_map: &mut SourceMap) {}
    fn visit_index_with(&mut self, _node_id: NodeId, _index_with: &mut IndexWith) {}
    fn visit_differentiate(&mut self, _node_id: NodeId, _differentiate: &mut Differentiate) {}
    fn visit_integrate(&mut self, _node_id: NodeId, _integrate: &mut Integrate) {}
    fn visit_delta0(&mut self, _node_id: NodeId, _delta0: &mut Delta0) {}
    fn visit_delayed_feedback(
        &mut self,
        _node_id: NodeId,
        _delayed_feedback: &mut DelayedFeedback,
    ) {
    }
    fn visit_distinct(&mut self, _node_id: NodeId, _distinct: &mut Distinct) {}
    fn visit_join_core(&mut self, _node_id: NodeId, _join_core: &mut JoinCore) {}
    fn visit_export(&mut self, _node_id: NodeId, _export: &mut Export) {}
    fn visit_exported_node(&mut self, _node_id: NodeId, _exported_node: &mut ExportedNode) {}
    fn visit_monotonic_join(&mut self, _node_id: NodeId, _monotonic_join: &mut MonotonicJoin) {}
    fn visit_constant(&mut self, _node_id: NodeId, _constant: &mut ConstantStream) {}
    fn visit_partitioned_rolling_fold(
        &mut self,
        _node_id: NodeId,
        _partitioned_rolling_fold: &mut PartitionedRollingFold,
    ) {
    }
    fn visit_flat_map(&mut self, _node_id: NodeId, _flat_map: &mut FlatMap) {}
    fn visit_antijoin(&mut self, _node_id: NodeId, _antijoin: &mut Antijoin) {}
    fn visit_index_by_column(&mut self, _node_id: NodeId, _index_by_column: &mut IndexByColumn) {}
    fn visit_unit_map_to_set(&mut self, _node_id: NodeId, _unit_map_to_set: &mut UnitMapToSet) {}
    fn visit_topk(&mut self, _node_id: NodeId, _topk: &mut TopK) {}

    fn visit_subgraph(&mut self, node_id: NodeId, subgraph: &mut Subgraph) {
        self.enter_subgraph(node_id, subgraph);
        for (&node_id, node) in subgraph.nodes_mut() {
            node.accept_mut(node_id, self);
        }
        self.leave_subgraph(node_id, subgraph);
    }
    fn enter_subgraph(&mut self, _node_id: NodeId, _subgraph: &mut Subgraph) {}
    fn leave_subgraph(&mut self, _node_id: NodeId, _subgraph: &mut Subgraph) {}
}

impl Node {
    pub fn accept<V>(&self, node_id: NodeId, visitor: &mut V)
    where
        V: NodeVisitor + ?Sized,
    {
        match self {
            Self::Map(map) => visitor.visit_map(node_id, map),
            Self::Min(min) => visitor.visit_min(node_id, min),
            Self::Max(max) => visitor.visit_max(node_id, max),
            Self::Neg(neg) => visitor.visit_neg(node_id, neg),
            Self::Sum(sum) => visitor.visit_sum(node_id, sum),
            Self::Fold(fold) => visitor.visit_fold(node_id, fold),
            Self::Sink(sink) => visitor.visit_sink(node_id, sink),
            Self::Minus(minus) => visitor.visit_minus(node_id, minus),
            Self::Filter(filter) => visitor.visit_filter(node_id, filter),
            Self::FilterMap(filter_map) => visitor.visit_filter_map(node_id, filter_map),
            Self::Source(source) => visitor.visit_source(node_id, source),
            Self::SourceMap(source_map) => visitor.visit_source_map(node_id, source_map),
            Self::IndexWith(index_with) => visitor.visit_index_with(node_id, index_with),
            Self::Differentiate(differentiate) => {
                visitor.visit_differentiate(node_id, differentiate);
            }
            Self::Integrate(integrate) => visitor.visit_integrate(node_id, integrate),
            Self::Delta0(delta0) => visitor.visit_delta0(node_id, delta0),
            Self::DelayedFeedback(delayed_feedback) => {
                visitor.visit_delayed_feedback(node_id, delayed_feedback);
            }
            Self::Distinct(distinct) => visitor.visit_distinct(node_id, distinct),
            Self::JoinCore(join_core) => visitor.visit_join_core(node_id, join_core),
            Self::Subgraph(subgraph) => visitor.visit_subgraph(node_id, subgraph),
            Self::Export(export) => visitor.visit_export(node_id, export),
            Self::ExportedNode(exported_node) => {
                visitor.visit_exported_node(node_id, exported_node);
            }
            Self::MonotonicJoin(monotonic_join) => {
                visitor.visit_monotonic_join(node_id, monotonic_join);
            }
            Self::ConstantStream(constant) => visitor.visit_constant(node_id, constant),
            Self::PartitionedRollingFold(partitioned_rolling_fold) => {
                visitor.visit_partitioned_rolling_fold(node_id, partitioned_rolling_fold);
            }
            Self::FlatMap(flat_map) => visitor.visit_flat_map(node_id, flat_map),
            Self::Antijoin(antijoin) => visitor.visit_antijoin(node_id, antijoin),
            Self::IndexByColumn(index_by_column) => {
                visitor.visit_index_by_column(node_id, index_by_column);
            }
            Self::UnitMapToSet(unit_map_to_set) => {
                visitor.visit_unit_map_to_set(node_id, unit_map_to_set);
            }
            Self::Topk(topk) => visitor.visit_topk(node_id, topk),
        }
    }

    pub fn accept_mut<V>(&mut self, node_id: NodeId, visitor: &mut V)
    where
        V: MutNodeVisitor + ?Sized,
    {
        match self {
            Self::Map(map) => visitor.visit_map(node_id, map),
            Self::Min(min) => visitor.visit_min(node_id, min),
            Self::Max(max) => visitor.visit_max(node_id, max),
            Self::Neg(neg) => visitor.visit_neg(node_id, neg),
            Self::Sum(sum) => visitor.visit_sum(node_id, sum),
            Self::Fold(fold) => visitor.visit_fold(node_id, fold),
            Self::Sink(sink) => visitor.visit_sink(node_id, sink),
            Self::Minus(minus) => visitor.visit_minus(node_id, minus),
            Self::Filter(filter) => visitor.visit_filter(node_id, filter),
            Self::FilterMap(filter_map) => visitor.visit_filter_map(node_id, filter_map),
            Self::Source(source) => visitor.visit_source(node_id, source),
            Self::SourceMap(source_map) => visitor.visit_source_map(node_id, source_map),
            Self::IndexWith(index_with) => visitor.visit_index_with(node_id, index_with),
            Self::Differentiate(differentiate) => {
                visitor.visit_differentiate(node_id, differentiate);
            }
            Self::Integrate(integrate) => visitor.visit_integrate(node_id, integrate),
            Self::Delta0(delta0) => visitor.visit_delta0(node_id, delta0),
            Self::DelayedFeedback(delayed_feedback) => {
                visitor.visit_delayed_feedback(node_id, delayed_feedback);
            }
            Self::Distinct(distinct) => visitor.visit_distinct(node_id, distinct),
            Self::JoinCore(join_core) => visitor.visit_join_core(node_id, join_core),
            Self::Subgraph(subgraph) => visitor.visit_subgraph(node_id, subgraph),
            Self::Export(export) => visitor.visit_export(node_id, export),
            Self::ExportedNode(exported_node) => {
                visitor.visit_exported_node(node_id, exported_node);
            }
            Self::MonotonicJoin(monotonic_join) => {
                visitor.visit_monotonic_join(node_id, monotonic_join);
            }
            Self::ConstantStream(constant) => visitor.visit_constant(node_id, constant),
            Self::PartitionedRollingFold(partitioned_rolling_fold) => {
                visitor.visit_partitioned_rolling_fold(node_id, partitioned_rolling_fold);
            }
            Self::FlatMap(flat_map) => visitor.visit_flat_map(node_id, flat_map),
            Self::Antijoin(antijoin) => visitor.visit_antijoin(node_id, antijoin),
            Self::IndexByColumn(index_by_column) => {
                visitor.visit_index_by_column(node_id, index_by_column);
            }
            Self::UnitMapToSet(unit_map_to_set) => {
                visitor.visit_unit_map_to_set(node_id, unit_map_to_set);
            }
            Self::Topk(topk) => visitor.visit_topk(node_id, topk),
        }
    }
}
