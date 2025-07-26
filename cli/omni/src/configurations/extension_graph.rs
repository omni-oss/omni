use std::{collections::HashMap, fmt::Debug, hash::Hash};

use merge::Merge;
use petgraph::{
    algo::is_cyclic_directed,
    graph::{DiGraph, EdgeIndex, NodeIndex},
    visit::{EdgeRef, IntoNodeReferences as _},
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

pub trait ExtensionGraphNode: Clone + Merge {
    type Id: Eq + Hash + Clone + Debug;

    fn id(&self) -> Self::Id;
}

pub trait ConnectedNodeIds: ExtensionGraphNode {
    fn connected_node_ids(&self) -> Vec<Self::Id>;
}

pub struct ExtensionGraph<T: Merge + ExtensionGraphNode> {
    index_map: HashMap<T::Id, NodeIndex>,
    di_graph: DiGraph<T, ()>,
}

impl<T: ExtensionGraphNode> ExtensionGraph<T> {
    pub fn new() -> Self {
        Self {
            index_map: HashMap::new(),
            di_graph: DiGraph::new(),
        }
    }
}

impl<T: Merge + ExtensionGraphNode> ExtensionGraph<T> {
    pub fn add_node(&mut self, node: T) -> ExtensionGraphResult<NodeIndex> {
        let id = node.id();

        if self.index_map.contains_key(&id) {
            Err(ExtensionGraphErrorInner::NodeAlreadyExists {
                message: format!("Node already exists: {id:?}"),
            })?;
        }

        let ni = self.di_graph.add_node(node);
        self.index_map.insert(id, ni);

        Ok(ni)
    }

    pub fn get_node_index(&self, id: &T::Id) -> Option<NodeIndex> {
        self.index_map.get(id).copied()
    }

    pub fn get_node(&self, id: &T::Id) -> Option<&T> {
        self.get_node_index(id)
            .and_then(|ni| self.di_graph.node_weight(ni))
    }

    pub fn add_edge(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
    ) -> ExtensionGraphResult<EdgeIndex> {
        let ei = self.di_graph.add_edge(to, from, ());

        if self.di_graph.contains_edge(to, from) {
            Err(ExtensionGraphErrorInner::EdgeAlreadyExists {
                message: format!("'{to:?}' -> '{from:?}'"),
            })?;
        }

        if is_cyclic_directed(&self.di_graph) {
            self.di_graph.remove_edge(ei);

            Err(ExtensionGraphErrorInner::CyclicDependency {
                message: format!("'{to:?}' -> '{from:?}'"),
            })?;
        }

        Ok(ei)
    }

    pub fn add_edge_by_id(
        &mut self,
        from: T::Id,
        to: T::Id,
    ) -> ExtensionGraphResult<EdgeIndex> {
        let from_index = self.get_node_index(&from).ok_or_else(|| {
            ExtensionGraphErrorInner::NodeNotFound {
                message: format!("Node not found: {from:?}"),
            }
        })?;
        let to_index = self.get_node_index(&to).ok_or_else(|| {
            ExtensionGraphErrorInner::NodeNotFound {
                message: format!("Node not found: {to:?}"),
            }
        })?;

        self.add_edge(from_index, to_index)
    }
}

impl<T: ConnectedNodeIds> ExtensionGraph<T> {
    pub fn connect_nodes(&mut self) -> ExtensionGraphResult<()> {
        // save connected nodes in case of error
        let edges = self
            .di_graph
            .edge_references()
            .map(|e| (e.source(), e.target()))
            .collect::<Vec<_>>();

        self.di_graph.clear_edges();

        let nodes = self
            .di_graph
            .node_references()
            .map(|(ni, node)| (ni, node.clone()))
            .collect::<Vec<_>>();

        for (ni, node) in nodes {
            let connected_node_ids = node.connected_node_ids();
            for connected_node_id in connected_node_ids {
                let other_ni = self
                    .get_node_index(&connected_node_id)
                    .ok_or_else(|| ExtensionGraphErrorInner::NodeNotFound {
                        message: format!(
                            "Node not found: {connected_node_id:?}"
                        ),
                    })?;

                self.add_edge(ni, other_ni)?;
            }
        }

        // put back edges after failure
        for (from, to) in edges {
            self.di_graph.add_edge(from, to, ());
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}Error: {inner}")]
pub struct ExtensionGraphError {
    kind: ExtensionGraphErrorKind,
    #[source]
    inner: ExtensionGraphErrorInner,
}

impl<T: Into<ExtensionGraphErrorInner>> From<T> for ExtensionGraphError {
    fn from(value: T) -> Self {
        let repr = value.into();
        let kind = repr.discriminant();
        Self { inner: repr, kind }
    }
}

impl ExtensionGraphError {
    pub fn kind(&self) -> ExtensionGraphErrorKind {
        self.kind
    }
}

pub type ExtensionGraphResult<T> = Result<T, ExtensionGraphError>;

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ExtensionGraphErrorKind), vis(pub))]
enum ExtensionGraphErrorInner {
    #[error("Cyclic dependency detected: {message}")]
    CyclicDependency { message: String },

    #[error("Node already exists: {message}")]
    NodeAlreadyExists { message: String },

    #[error("Node not found: {message}")]
    NodeNotFound { message: String },

    #[error("Edge already exists: {message}")]
    EdgeAlreadyExists { message: String },
}
