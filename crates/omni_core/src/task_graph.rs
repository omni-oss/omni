use std::collections::HashMap;

use petgraph::graph::{DiGraph, NodeIndex};

use crate::Task;

#[derive(Debug, Default)]
pub struct TaskGraph {
    _node_map: HashMap<String, NodeIndex>,
    _di_graph: DiGraph<Task, ()>,
}
