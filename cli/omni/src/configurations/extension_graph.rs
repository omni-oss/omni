use std::{collections::HashMap, fmt::Debug, hash::Hash};

use merge::Merge;
use petgraph::{
    Direction,
    algo::is_cyclic_directed,
    graph::{DiGraph, EdgeIndex, NodeIndex},
    visit::{EdgeRef, IntoNodeReferences as _},
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

pub trait ExtensionGraphNode: Clone + Merge + Debug {
    type Id: Eq + Hash + Clone + Debug;

    fn id(&self) -> &Self::Id;
    fn extendee_ids(&self) -> &[Self::Id];
}

pub struct ExtensionGraph<T: Merge + ExtensionGraphNode> {
    index_map: HashMap<T::Id, NodeIndex>,
    di_graph: DiGraph<T, ()>,
    processed_nodes: HashMap<T::Id, T>,
}

impl<T: ExtensionGraphNode> ExtensionGraph<T> {
    pub fn new() -> Self {
        Self {
            index_map: HashMap::new(),
            di_graph: DiGraph::new(),
            processed_nodes: HashMap::new(),
        }
    }
}

impl<T: Merge + ExtensionGraphNode> ExtensionGraph<T> {
    pub fn add_node(&mut self, node: T) -> ExtensionGraphResult<NodeIndex> {
        let id = node.id().clone();

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
        extender: NodeIndex,
        extendee: NodeIndex,
    ) -> ExtensionGraphResult<EdgeIndex> {
        if self.di_graph.contains_edge(extendee, extender) {
            Err(ExtensionGraphErrorInner::EdgeAlreadyExists {
                message: format!("'{extendee:?}' -> '{extender:?}'"),
            })?;
        }

        let ei = self.di_graph.add_edge(extendee, extender, ());

        if is_cyclic_directed(&self.di_graph) {
            self.di_graph.remove_edge(ei);

            Err(ExtensionGraphErrorInner::CyclicDependency {
                message: format!("'{extendee:?}' -> '{extender:?}'"),
            })?;
        }

        Ok(ei)
    }

    pub fn add_edge_by_id(
        &mut self,
        extender: T::Id,
        extendee: T::Id,
    ) -> ExtensionGraphResult<EdgeIndex> {
        let extender = self.get_node_index(&extender).ok_or_else(|| {
            ExtensionGraphErrorInner::NodeNotFound {
                message: format!("Node not found: {extender:?}"),
            }
        })?;
        let extendee = self.get_node_index(&extendee).ok_or_else(|| {
            ExtensionGraphErrorInner::NodeNotFound {
                message: format!("Node not found: {extendee:?}"),
            }
        })?;

        self.add_edge(extender, extendee)
    }

    pub fn connect_nodes(&mut self) -> ExtensionGraphResult<()> {
        // save connected nodes in case of error, this is to keep the graph in a consistent state
        // if an error occurs
        let saved_edges = self
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

        for (extender, node) in nodes {
            let extendee_ids = node.extendee_ids();

            for extendee_id in extendee_ids {
                let extendee =
                    self.get_node_index(extendee_id).ok_or_else(|| {
                        ExtensionGraphErrorInner::NodeNotFound {
                            message: format!("Node not found: {extendee_id:?}"),
                        }
                    })?;

                self.add_edge(extender, extendee)?;
            }
        }

        // put back edges after failure, this is to keep the graph in a consistent state
        for (a, b) in saved_edges {
            self.di_graph.add_edge(a, b, ());
        }

        Ok(())
    }

    pub fn process_node<'a>(
        &'a mut self,
        id: &T::Id,
    ) -> ExtensionGraphResult<&'a T> {
        let node = self.get_node_index(id).ok_or_else(|| {
            ExtensionGraphErrorInner::NodeNotFound {
                message: format!("'{id:?}'"),
            }
        })?;
        let extendee_indices = self
            .di_graph
            .edges_directed(node, Direction::Incoming)
            .map(|e| e.source())
            .collect::<Vec<_>>();

        let mut own_processed_nodes = HashMap::new();

        if !extendee_indices.is_empty() {
            for edge in extendee_indices {
                let extendee_id = self.di_graph[edge].id().clone();
                let extendee = if let Some(node) =
                    self.processed_nodes.get(&extendee_id).cloned()
                {
                    node
                } else {
                    self.process_node(&extendee_id)?.clone()
                };
                own_processed_nodes.insert(extendee_id, extendee);
            }
        }

        let mut node = self.di_graph[node].clone();

        if !own_processed_nodes.is_empty() {
            let ids = node.extendee_ids().to_vec();

            for id in ids {
                if let Some(extended_node) = own_processed_nodes.remove(&id) {
                    node.merge(extended_node.clone());
                }
            }
        }

        self.processed_nodes.insert(id.clone(), node);

        Ok(self
            .processed_nodes
            .get(id)
            .expect("At this point node should exist"))
    }

    pub fn get_processed_node<'a>(&'a self, id: &T::Id) -> Option<&'a T> {
        self.processed_nodes.get(id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Merge, Clone, Debug)]
    struct TestNode {
        #[merge(skip)]
        id: i32,

        #[merge(skip)]
        extends: Vec<i32>,

        #[merge(strategy = merge::vec::prepend)]
        items: Vec<i32>,
    }

    impl ExtensionGraphNode for TestNode {
        type Id = i32;

        fn id(&self) -> &Self::Id {
            &self.id
        }

        fn extendee_ids(&self) -> &[Self::Id] {
            &self.extends
        }
    }

    #[test]
    fn test_get_processed_node_linear_extensions() {
        let mut graph = ExtensionGraph::new();

        graph
            .add_node(TestNode {
                id: 1,
                extends: vec![],
                items: vec![1, 2, 3],
            })
            .expect("Can't add node");

        graph
            .add_node(TestNode {
                id: 2,
                extends: vec![1],
                items: vec![4, 5],
            })
            .expect("Can't add node");

        graph
            .add_node(TestNode {
                id: 3,
                extends: vec![2],
                items: vec![6, 7],
            })
            .expect("Can't add node");

        graph.connect_nodes().expect("Can't connect nodes");
        let processed_node =
            graph.process_node(&3).expect("Can't get processed node");

        assert_eq!(processed_node.items, vec![1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn test_get_processed_node_multiple_extensions() {
        let mut graph = ExtensionGraph::new();

        graph
            .add_node(TestNode {
                id: 1,
                extends: vec![],
                items: vec![1, 2, 3],
            })
            .expect("Can't add node");

        graph
            .add_node(TestNode {
                id: 2,
                extends: vec![1],
                items: vec![4, 5],
            })
            .expect("Can't add node");

        graph
            .add_node(TestNode {
                id: 3,
                extends: vec![1, 2],
                items: vec![6, 7],
            })
            .expect("Can't add node");

        graph.connect_nodes().expect("Can't connect nodes");
        let processed_node =
            graph.process_node(&3).expect("Can't get processed node");

        assert_eq!(processed_node.items, vec![1, 2, 3, 4, 5, 6, 7]);
    }
}
