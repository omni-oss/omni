use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::{DefaultHasher, Hash, Hasher as _},
};

use merge::Merge;
use petgraph::{
    algo::is_cyclic_directed,
    graph::{DiGraph, EdgeIndex, NodeIndex},
    visit::{Dfs, IntoNodeReferences as _, Reversed},
};
use strum::{EnumDiscriminants, IntoDiscriminant as _};

pub trait ExtensionGraphNode: Clone + Merge + Debug {
    type Id: Eq + Hash + Clone + Debug;

    fn id(&self) -> &Self::Id;

    #[inline(always)]
    /// Implement this method if merge for this is skipped
    fn set_id(&mut self, id: &Self::Id) {
        _ = id;
    }
    fn extendee_ids(&self) -> &[Self::Id];
    #[inline(always)]
    /// Implement this method if merge for this is skipped
    fn set_extendee_ids(&mut self, extendee_ids: &[Self::Id]) {
        _ = extendee_ids;
    }
}

pub struct ExtensionGraph<T: Merge + ExtensionGraphNode> {
    index_map: HashMap<T::Id, NodeIndex>,
    di_graph: DiGraph<T, ()>,
    path_traversals: HashMap<T::Id, PathTraversalKey>,
    processed_nodes: HashMap<PathTraversalKey, T>,
}

impl<T: ExtensionGraphNode> ExtensionGraph<T> {
    pub fn new() -> Self {
        Self {
            index_map: HashMap::new(),
            di_graph: DiGraph::new(),
            path_traversals: HashMap::new(),
            processed_nodes: HashMap::new(),
        }
    }

    pub fn from_nodes(nodes: Vec<T>) -> ExtensionGraphResult<Self> {
        let mut graph = Self::new();

        for node in nodes {
            graph.add_node(node)?;
        }

        graph.connect_nodes()?;

        Ok(graph)
    }
}

impl<T: Merge + ExtensionGraphNode> ExtensionGraph<T> {
    pub fn get_or_process_all_nodes(&mut self) -> ExtensionGraphResult<Vec<T>> {
        let ids = self.get_all_node_ids();
        let mut nodes = vec![];

        for id in ids {
            let node = if let Some(node) = self.get_processed_node(&id) {
                node
            } else {
                self.process_node_by_id(&id)?
            };
            nodes.push(node.clone());
        }

        Ok(nodes)
    }

    fn get_all_node_ids(&self) -> Vec<T::Id> {
        let mut ids = vec![];

        for ni in self.di_graph.node_indices() {
            let id = self.di_graph[ni].id().clone();
            ids.push(id);
        }

        ids
    }

    fn add_node(&mut self, node: T) -> ExtensionGraphResult<NodeIndex> {
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

    fn get_node_index(&self, id: &T::Id) -> Option<NodeIndex> {
        self.index_map.get(id).copied()
    }

    fn get_node(&self, id: &T::Id) -> Option<&T> {
        self.get_node_index(id)
            .and_then(|ni| self.di_graph.node_weight(ni))
    }

    fn add_edge(
        &mut self,
        extender: NodeIndex,
        extendee: NodeIndex,
    ) -> ExtensionGraphResult<EdgeIndex> {
        let ei = self.di_graph.add_edge(extendee, extender, ());

        if is_cyclic_directed(&self.di_graph) {
            self.di_graph.remove_edge(ei);

            Err(ExtensionGraphErrorInner::CyclicDependency {
                message: format!("'{extendee:?}' -> '{extender:?}'"),
            })?;
        }

        Ok(ei)
    }

    fn add_edge_by_id(
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

    fn connect_nodes(&mut self) -> ExtensionGraphResult<()> {
        self.di_graph.clear_edges();

        let nodes = self
            .di_graph
            .node_references()
            .map(|(ni, node)| (ni, node.clone()))
            .collect::<Vec<_>>();

        for (extender_idx, node) in nodes {
            let extendee_ids = node.extendee_ids();
            let num_extendees = extendee_ids.len();

            // Linearize the extension graph, starting from the most extending node
            let mut current_extender_idx = extender_idx;
            for i in (0..num_extendees).rev() {
                let extendee_id = &extendee_ids[i];
                let extendee_idx = self
                    .get_node_index(extendee_id)
                    .ok_or_else(|| ExtensionGraphErrorInner::NodeNotFound {
                        message: format!("Node not found: {i:?}"),
                    })?;

                self.add_edge(current_extender_idx, extendee_idx)?;

                current_extender_idx = extendee_idx;
            }
        }

        Ok(())
    }

    pub fn process_node_by_id<'a>(
        &'a mut self,
        id: &T::Id,
    ) -> ExtensionGraphResult<&'a T> {
        // if we've already processed this node, return it
        if let Some(path) = self.path_traversals.get(id) {
            return Ok(self
                .processed_nodes
                .get(path)
                .expect("should be able to get processed node"));
        }

        let node = self.get_node_index(id).ok_or_else(|| {
            ExtensionGraphErrorInner::NodeNotFound {
                message: format!("'{id:?}'"),
            }
        })?;

        let graph = Reversed(&self.di_graph);
        let mut dfs = Dfs::new(graph, node);

        let mut node_indices = vec![];
        while let Some(node) = dfs.next(graph) {
            node_indices.push(node);
        }

        node_indices.reverse();

        if self.cache_exists_by_path(&node_indices) {
            if !self.cached_path_exists(id) {
                self.cache_path(id, &node_indices);
            }

            return Ok(self
                .get_processed_node_by_path(&node_indices)
                .expect("Should be able to get processed node"));
        }

        let total_nodes = node_indices.len();
        let mut prev_processed_node: Option<T> = None;
        let mut visited = HashSet::<NodeIndex>::new();

        for i in 0..total_nodes {
            let path = &node_indices[..=i];
            let node_index = node_indices[i];

            // if we've already visited this node, don't process it entirely
            if visited.contains(&node_index) {
                continue;
            }

            let current_node = self.di_graph[node_index].clone();

            let resulting_node = if let Some(node) =
                self.get_processed_node_by_path(path).cloned()
            {
                node
            } else if i == 0 {
                current_node
            } else {
                let mut merged_node = prev_processed_node
                    .clone()
                    .expect("Should have a previous node");
                merged_node.set_id(current_node.id());
                merged_node.set_extendee_ids(current_node.extendee_ids());
                merged_node.merge(current_node);

                merged_node
            };

            visited.insert(node_index);
            if !self.cache_exists_by_path(path) {
                self.cache_by_path(path, resulting_node.clone());
            }
            prev_processed_node = Some(resulting_node);
        }

        // save the path to the cache for future lookups
        self.cache_path(id, &node_indices);

        Ok(self
            .get_processed_node(id)
            .expect("At this point, the resulting node should exist"))
    }

    fn cache_exists_by_path(&self, path_traversal: &[NodeIndex]) -> bool {
        let key = PathTraversalKey::new(path_traversal);

        self.processed_nodes.contains_key(&key)
    }

    fn cache_by_path(&mut self, path_traversal: &[NodeIndex], node: T) {
        let key = PathTraversalKey::new(path_traversal);
        self.processed_nodes.insert(key, node);
    }

    fn cached_path_exists(&self, node_id: &T::Id) -> bool {
        self.path_traversals.contains_key(node_id)
    }

    fn cache_path(&mut self, node_id: &T::Id, path_traversal: &[NodeIndex]) {
        let key = PathTraversalKey::new(path_traversal);
        self.path_traversals.insert(node_id.clone(), key);
    }

    fn get_processed_node<'a>(&'a self, id: &T::Id) -> Option<&'a T> {
        let key = self.path_traversals.get(id).copied()?;

        self.processed_nodes.get(&key)
    }

    fn get_processed_node_by_path<'a>(
        &'a self,
        path_traversal: &[NodeIndex],
    ) -> Option<&'a T> {
        let key = PathTraversalKey::new(path_traversal);

        self.processed_nodes.get(&key)
    }
}

#[derive(Hash, PartialEq, Eq, Clone, Copy, Debug)]
struct PathTraversalKey(u64);

impl PathTraversalKey {
    pub fn new(path: &[NodeIndex]) -> Self {
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let hashed = hasher.finish();

        Self(hashed)
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
    #[error("cyclic dependency detected: {message}")]
    CyclicDependency { message: String },

    #[error("node already exists: {message}")]
    NodeAlreadyExists { message: String },

    #[error("node not found: {message}")]
    NodeNotFound { message: String },

    #[error(transparent)]
    Unknown(#[from] eyre::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Merge, Clone, Debug, PartialEq, Eq)]
    struct TestNode {
        #[merge(skip)]
        id: i32,

        #[merge(skip)]
        extends: Vec<i32>,

        #[merge(strategy = merge::vec::append)]
        items: Vec<i32>,
    }

    impl ExtensionGraphNode for TestNode {
        type Id = i32;

        fn id(&self) -> &Self::Id {
            &self.id
        }

        fn set_id(&mut self, id: &Self::Id) {
            self.id = id.clone();
        }

        fn extendee_ids(&self) -> &[Self::Id] {
            &self.extends
        }

        fn set_extendee_ids(&mut self, extendee_ids: &[Self::Id]) {
            self.extends = extendee_ids.to_vec();
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
        let processed_node = graph
            .process_node_by_id(&3)
            .expect("Can't get processed node")
            .clone();

        assert_eq!(
            processed_node,
            TestNode {
                id: 3,
                extends: vec![2],
                items: vec![1, 2, 3, 4, 5, 6, 7]
            }
        );
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
                extends: vec![2],
                items: vec![6, 7],
            })
            .expect("Can't add node");

        graph
            .add_node(TestNode {
                id: 4,
                extends: vec![1, 2, 3],
                items: vec![8, 9],
            })
            .expect("Can't add node");

        graph.connect_nodes().expect("Can't connect nodes");
        let processed_node = graph
            .process_node_by_id(&4)
            .expect("Can't get processed node")
            .clone();

        assert_eq!(
            processed_node,
            TestNode {
                id: 4,
                extends: vec![1, 2, 3],
                items: vec![1, 2, 3, 4, 5, 6, 7, 8, 9]
            }
        );
    }
}
