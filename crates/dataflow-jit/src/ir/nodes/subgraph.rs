use crate::ir::{
    function::Function,
    graph::{self, GraphExt},
    layout_cache::RowLayoutCache,
    nodes::{DataflowNode, DelayedFeedback, Delta0, Export, Node, StreamLayout},
    LayoutId, NodeId,
};
use petgraph::prelude::DiGraphMap;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct Subgraph {
    // A map of external nodes to their subgraph import nodes
    inputs: BTreeMap<NodeId, NodeId>,
    subgraph: graph::Subgraph,
    // A map of subgraph nodes to their export nodes
    outputs: BTreeMap<NodeId, NodeId>,
    feedback: BTreeSet<NodeId>,
    // Connections from streams to a feedback node within `feedback`
    feedback_connections: BTreeMap<NodeId, NodeId>,
}

impl Subgraph {
    pub fn new(subgraph: graph::Subgraph) -> Self {
        Self {
            inputs: BTreeMap::new(),
            subgraph,
            outputs: BTreeMap::new(),
            feedback: BTreeSet::new(),
            feedback_connections: BTreeMap::new(),
        }
    }

    pub const fn subgraph(&self) -> &graph::Subgraph {
        &self.subgraph
    }

    pub fn subgraph_mut(&mut self) -> &mut graph::Subgraph {
        &mut self.subgraph
    }

    pub fn delta0<L>(&mut self, import: NodeId, layout: L) -> NodeId
    where
        L: Into<StreamLayout>,
    {
        let delta0 = self.add_node(Delta0::new(import, layout.into()));
        self.inputs.insert(import, delta0);
        delta0
    }

    pub fn export(&mut self, output: NodeId, layout: StreamLayout) -> NodeId {
        let export = self.add_node(Export::new(output, layout));
        self.outputs.insert(output, export);
        export
    }

    pub fn delayed_feedback(&mut self, layout: LayoutId) -> NodeId {
        let delay = self.add_node(DelayedFeedback::new(layout));
        self.feedback.insert(delay);
        delay
    }

    pub fn connect_feedback(&mut self, from: NodeId, feedback: NodeId) {
        debug_assert!(self.feedback.contains(&feedback));
        self.feedback_connections.insert(from, feedback);
    }

    pub(crate) fn input_nodes(&self) -> &BTreeMap<NodeId, NodeId> {
        &self.inputs
    }

    pub(crate) fn output_nodes(&self) -> &BTreeMap<NodeId, NodeId> {
        &self.outputs
    }

    pub(crate) fn feedback_connections(&self) -> &BTreeMap<NodeId, NodeId> {
        &self.feedback_connections
    }
}

impl GraphExt for Subgraph {
    fn layout_cache(&self) -> &RowLayoutCache {
        self.subgraph.layout_cache()
    }

    fn nodes(&self) -> &BTreeMap<NodeId, Node> {
        self.subgraph.nodes()
    }

    fn nodes_mut(&mut self) -> &mut BTreeMap<NodeId, Node> {
        self.subgraph.nodes_mut()
    }

    fn next_node(&self) -> NodeId {
        self.subgraph.next_node()
    }

    fn create_node<N>(&mut self, node_id: NodeId, node: N) -> NodeId
    where
        N: Into<Node>,
    {
        self.subgraph.create_node(node_id, node)
    }

    fn subgraph<F, T>(&mut self, build: F) -> (NodeId, T)
    where
        F: FnOnce(&mut Subgraph) -> T,
    {
        self.subgraph.subgraph(build)
    }

    fn optimize(&mut self) {
        self.subgraph.optimize();
    }

    fn edges(&self) -> &DiGraphMap<NodeId, ()> {
        self.subgraph.edges()
    }

    fn edges_mut(&mut self) -> &mut DiGraphMap<NodeId, ()> {
        self.subgraph.edges_mut()
    }
}

impl DataflowNode for Subgraph {
    fn map_inputs<F>(&self, map: &mut F)
    where
        F: FnMut(NodeId) + ?Sized,
    {
        self.inputs.keys().copied().for_each(map);
    }

    fn map_inputs_mut<F>(&mut self, map: &mut F)
    where
        F: FnMut(&mut NodeId) + ?Sized,
    {
        self.subgraph_mut().map_inputs_mut_inner(map);
        // TODO: Probably need to map over the feedback nodes as well
    }

    fn output_stream(
        &self,
        _inputs: &[StreamLayout],
        _layout_cache: &RowLayoutCache,
    ) -> Option<StreamLayout> {
        None
    }

    fn validate(&self, _inputs: &[StreamLayout], _layout_cache: &RowLayoutCache) {}

    fn optimize(&mut self, _layout_cache: &RowLayoutCache) {
        self.subgraph_mut().optimize();
    }

    fn functions<'a>(&'a self, functions: &mut Vec<&'a Function>) {
        self.subgraph.functions(functions);
    }

    fn map_layouts<F>(&self, map: &mut F)
    where
        F: FnMut(LayoutId) + ?Sized,
    {
        self.subgraph.map_layouts_inner(map)
    }

    fn remap_layouts(&mut self, mappings: &BTreeMap<LayoutId, LayoutId>) {
        self.subgraph.remap_layouts(mappings);
    }
}
