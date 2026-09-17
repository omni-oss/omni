use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use omni_configurations::{SourceConfig, SourceConfigProfile};
use url::Url;

use crate::error::Error;

/// Backstop bound on how deeply meta sources may nest before expansion aborts.
///
/// Cycle detection is the primary guard; this cap only catches a graph that
/// cycle detection cannot see (for example a repository that references itself
/// at ever-changing revisions). It is deliberately lower than omni's in-process
/// nesting limits because each meta descent is a network fetch.
pub const DEFAULT_META_PROJECTION_DEPTH: usize = 16;

/// The identity of a materialized source, used to detect a source that is its
/// own ancestor along the current traversal path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SourceIdentity {
    Git { uri: Url, rev: String },
    Local(PathBuf),
}

/// The classification of a materialized source: an optional resolved payload
/// the node contributes itself, together with any member sources to expand
/// beneath it. A node may be leaf-only (`leaf` set, `children` empty),
/// meta-only (`leaf` `None`, `children` populated), or both at once. Whether a
/// given subsystem ever produces a both-and node is that subsystem's choice;
/// the projection subsystem, for instance, only ever produces leaf-only or
/// meta-only nodes, so its behavior is unchanged by the both-and capability.
#[derive(Debug)]
pub struct Node<Leaf, P: SourceConfigProfile> {
    /// The payload this node contributes directly, if any.
    pub leaf: Option<Leaf>,
    /// Member sources to expand beneath this node, if any.
    pub children: Vec<SourceConfig<P>>,
}

impl<Leaf, P: SourceConfigProfile> Node<Leaf, P> {
    /// A node that only contributes its own payload (no member sources).
    pub fn leaf(leaf: Leaf) -> Self {
        Self {
            leaf: Some(leaf),
            children: Vec::new(),
        }
    }

    /// A node that only bundles member sources (contributes no payload itself).
    pub fn meta(children: Vec<SourceConfig<P>>) -> Self {
        Self {
            leaf: None,
            children,
        }
    }

    /// A node that both contributes its own payload and bundles member sources.
    pub fn both(leaf: Leaf, children: Vec<SourceConfig<P>>) -> Self {
        Self {
            leaf: Some(leaf),
            children,
        }
    }
}

/// The result of materializing (fetching/pinning) and classifying one source.
#[derive(Debug)]
pub struct Materialized<Leaf, P: SourceConfigProfile> {
    pub node: Node<Leaf, P>,
    pub identity: SourceIdentity,
    pub root: PathBuf,
    pub pin: Option<String>,
}

/// A fully expanded leaf source ready for the caller to consume.
///
/// `id` is the authored id (used for on-disk placement); `qualified_id` is the
/// system-composed identity (`<parent>::<child>`) used for the ledger,
/// uniqueness, garbage collection, and teardown.
#[derive(Debug)]
pub struct EffectiveSource<Leaf> {
    pub id: String,
    pub qualified_id: String,
    pub root: PathBuf,
    pub pin: Option<String>,
    pub leaf: Leaf,
}

/// Materialize and classify one source. All I/O lives behind this trait so the
/// traversal itself is pure and unit-testable with a fake implementation.
pub trait MetaExpand {
    /// The source-config profile carried by each source (e.g. the projection or
    /// pack profile).
    type Profile: SourceConfigProfile;
    /// The resolved payload a leaf source contributes.
    type Leaf;
    /// The caller's error type; it must absorb this crate's traversal errors.
    type Error: From<Error>;

    #[allow(async_fn_in_trait)]
    async fn classify(
        &self,
        src: &SourceConfig<Self::Profile>,
        qualified_id: &str,
        parent_root: &Path,
        depth: usize,
    ) -> Result<Materialized<Self::Leaf, Self::Profile>, Self::Error>;

    /// The authored id of a member source, read without any I/O.
    fn declared_id<'a>(&self, src: &'a SourceConfig<Self::Profile>) -> &'a str;
}

/// Return the first `::`-delimited segment of a selection argument.
pub fn first_segment(arg: &str) -> &str {
    match arg.split_once("::") {
        Some((head, _)) => head,
        None => arg,
    }
}

/// A qualified id matches a selection argument when it is exactly the argument
/// or a descendant of it (`arg::...`). The `::` anchor prevents a false prefix
/// match such as `org` against `org-standard`.
pub fn matches(qualified_id: &str, arg: &str) -> bool {
    qualified_id == arg
        || qualified_id
            .strip_prefix(arg)
            .is_some_and(|rest| rest.starts_with("::"))
}

/// A source at `qualified_id` should be traversed for a given selection when it
/// is inside the selected subtree or lies on the path down to it.
pub fn selected_or_on_path(qualified_id: &str, arg: &str) -> bool {
    matches(qualified_id, arg) || matches(arg, qualified_id)
}

fn compose_qualified_id(prefix: Option<&str>, id: &str) -> String {
    match prefix {
        Some(prefix) => format!("{prefix}::{id}"),
        None => id.to_string(),
    }
}

struct Frame<P: SourceConfigProfile> {
    src: SourceConfig<P>,
    qualified_id: String,
    parent_root: PathBuf,
    depth: usize,
    ancestors: Vec<(SourceIdentity, String)>,
}

fn detect_duplicates<X>(
    expander: &X,
    siblings: &[SourceConfig<X::Profile>],
    bundle: Option<&str>,
) -> Result<(), Error>
where
    X: MetaExpand,
{
    let mut seen: HashSet<&str> = HashSet::new();
    for src in siblings {
        let id = expander.declared_id(src);
        if !seen.insert(id) {
            return Err(Error::duplicate_member_id(bundle, id));
        }
    }
    Ok(())
}

fn push_children<P: SourceConfigProfile>(
    stack: &mut Vec<Frame<P>>,
    children: Vec<SourceConfig<P>>,
    child_qualified_ids: Vec<String>,
    parent_root: PathBuf,
    depth: usize,
    ancestors: Vec<(SourceIdentity, String)>,
) {
    // Push in reverse so the stack pops children in declared order (pre-order).
    for (src, qualified_id) in
        children.into_iter().zip(child_qualified_ids).rev()
    {
        stack.push(Frame {
            src,
            qualified_id,
            parent_root: parent_root.clone(),
            depth,
            ancestors: ancestors.clone(),
        });
    }
}

fn cycle_chain(
    ancestors: &[(SourceIdentity, String)],
    current: &str,
) -> String {
    let mut parts: Vec<&str> =
        ancestors.iter().map(|(_, qid)| qid.as_str()).collect();
    parts.push(current);
    parts.join(" → ")
}

/// Expand a list of top-level sources into their effective leaf sources.
///
/// The traversal is a deterministic pre-order depth-first walk over the
/// declared source lists. Meta sources recurse; cycle detection (primary) and
/// the depth cap (backstop) keep it total. When `select` is set, only the
/// matching subtree is traversed, so unrelated sources are never materialized.
pub async fn expand<X>(
    expander: &X,
    sources: &[SourceConfig<X::Profile>],
    root: &Path,
    max_depth: usize,
    select: Option<&str>,
) -> Result<Vec<EffectiveSource<X::Leaf>>, X::Error>
where
    X: MetaExpand,
{
    detect_duplicates(expander, sources, None)?;

    let mut stack: Vec<Frame<X::Profile>> = Vec::new();
    let child_qids: Vec<String> = sources
        .iter()
        .map(|s| compose_qualified_id(None, expander.declared_id(s)))
        .collect();
    push_children(
        &mut stack,
        sources.to_vec(),
        child_qids,
        root.to_path_buf(),
        0,
        Vec::new(),
    );

    let mut out: Vec<EffectiveSource<X::Leaf>> = Vec::new();

    while let Some(frame) = stack.pop() {
        let qualified_id = frame.qualified_id;

        if let Some(select) = select {
            if !selected_or_on_path(&qualified_id, select) {
                continue;
            }
        }

        if frame.depth > max_depth {
            return Err(Error::depth_exceeded(max_depth, qualified_id).into());
        }

        let materialized = expander
            .classify(
                &frame.src,
                &qualified_id,
                &frame.parent_root,
                frame.depth,
            )
            .await?;

        if frame
            .ancestors
            .iter()
            .any(|(identity, _)| *identity == materialized.identity)
        {
            return Err(Error::cycle_detected(cycle_chain(
                &frame.ancestors,
                &qualified_id,
            ))
            .into());
        }

        let Node { leaf, children } = materialized.node;

        // A node contributes its own payload (if any) at its qualified id.
        if let Some(leaf) = leaf {
            let selected = select.is_none_or(|s| matches(&qualified_id, s));
            if selected {
                out.push(EffectiveSource {
                    id: expander.declared_id(&frame.src).to_string(),
                    qualified_id: qualified_id.clone(),
                    root: materialized.root.clone(),
                    pin: materialized.pin,
                    leaf,
                });
            }
        }

        // Then it expands its member sources (if any) beneath it. A leaf-only
        // node has no children and stops here; a meta-only node contributed no
        // payload above; a both-and node does both, in that order (pre-order).
        if !children.is_empty() {
            detect_duplicates(expander, &children, Some(&qualified_id))?;

            let child_qids: Vec<String> = children
                .iter()
                .map(|s| {
                    compose_qualified_id(
                        Some(&qualified_id),
                        expander.declared_id(s),
                    )
                })
                .collect();

            let mut ancestors = frame.ancestors;
            ancestors.push((materialized.identity, qualified_id));

            push_children(
                &mut stack,
                children,
                child_qids,
                materialized.root,
                frame.depth + 1,
                ancestors,
            );
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap};

    use omni_configurations::types::SingleOrMany;
    use omni_configurations::{
        GitSource, LocalSource, ProjectionId, ProjectionProfile,
        ProjectionRoutes,
    };

    use super::*;

    fn git(id: &str, uri: &str, rev: &str) -> SourceConfig<ProjectionProfile> {
        SourceConfig::Git(GitSource {
            uri: Url::parse(uri).unwrap(),
            rev: rev.to_string(),
            base: ProjectionRoutes::default(),
            extra: ProjectionId { id: id.to_string() },
        })
    }

    fn local(id: &str, path: &str) -> SourceConfig<ProjectionProfile> {
        SourceConfig::Local(LocalSource {
            path: SingleOrMany::Single(path.to_string()),
            base: ProjectionRoutes::default(),
            extra: ProjectionId { id: id.to_string() },
        })
    }

    enum FakeNode {
        Leaf(String),
        Meta(Vec<SourceConfig<ProjectionProfile>>),
        Both(String, Vec<SourceConfig<ProjectionProfile>>),
    }

    struct Fake {
        nodes: HashMap<String, FakeNode>,
        classified: RefCell<Vec<String>>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                nodes: HashMap::new(),
                classified: RefCell::new(Vec::new()),
            }
        }

        fn leaf(mut self, id: &str) -> Self {
            self.nodes.insert(
                id.to_string(),
                FakeNode::Leaf(format!("payload:{id}")),
            );
            self
        }

        fn meta(
            mut self,
            id: &str,
            children: Vec<SourceConfig<ProjectionProfile>>,
        ) -> Self {
            self.nodes.insert(id.to_string(), FakeNode::Meta(children));
            self
        }

        fn both(
            mut self,
            id: &str,
            children: Vec<SourceConfig<ProjectionProfile>>,
        ) -> Self {
            self.nodes.insert(
                id.to_string(),
                FakeNode::Both(format!("payload:{id}"), children),
            );
            self
        }
    }

    fn local_path(src: &SourceConfig<ProjectionProfile>) -> String {
        match src {
            SourceConfig::Local(l) => match &l.path {
                SingleOrMany::Single(p) => p.clone(),
                SingleOrMany::Many(ps) => {
                    ps.first().cloned().unwrap_or_default()
                }
            },
            SourceConfig::Git(_) | SourceConfig::Registry(_) => String::new(),
        }
    }

    impl MetaExpand for Fake {
        type Profile = ProjectionProfile;
        type Leaf = String;
        type Error = Error;

        async fn classify(
            &self,
            src: &SourceConfig<Self::Profile>,
            qualified_id: &str,
            parent_root: &Path,
            _depth: usize,
        ) -> Result<Materialized<Self::Leaf, Self::Profile>, Self::Error>
        {
            self.classified.borrow_mut().push(qualified_id.to_string());

            let id = self.declared_id(src).to_string();
            let identity = match src {
                SourceConfig::Git(g) => SourceIdentity::Git {
                    uri: g.uri.clone(),
                    rev: g.rev.clone(),
                },
                SourceConfig::Local(_) => {
                    SourceIdentity::Local(PathBuf::from(local_path(src)))
                }
                SourceConfig::Registry(_) => {
                    SourceIdentity::Local(PathBuf::from(local_path(src)))
                }
            };

            let node = match self.nodes.get(&id) {
                Some(FakeNode::Leaf(p)) => Node::leaf(p.clone()),
                Some(FakeNode::Meta(children)) => Node::meta(children.clone()),
                Some(FakeNode::Both(p, children)) => {
                    Node::both(p.clone(), children.clone())
                }
                None => Node::leaf(format!("payload:{id}")),
            };

            Ok(Materialized {
                node,
                identity,
                root: parent_root.join(&id),
                pin: None,
            })
        }

        fn declared_id<'a>(
            &self,
            src: &'a SourceConfig<Self::Profile>,
        ) -> &'a str {
            match src {
                SourceConfig::Git(g) => &g.extra.id,
                SourceConfig::Local(l) => &l.extra.id,
                SourceConfig::Registry(_) => "",
            }
        }
    }

    fn root() -> PathBuf {
        PathBuf::from("/ws")
    }

    async fn expand_all(
        fake: &Fake,
        sources: &[SourceConfig<ProjectionProfile>],
    ) -> Result<Vec<EffectiveSource<String>>, Error> {
        expand(fake, sources, &root(), DEFAULT_META_PROJECTION_DEPTH, None)
            .await
    }

    #[test]
    fn matches_is_anchored_on_the_delimiter() {
        assert!(matches("org", "org"));
        assert!(matches("org::skills", "org"));
        assert!(matches("org::skills", "org::skills"));
        assert!(!matches("org-standard", "org"));
        assert!(!matches("org", "org::skills"));
    }

    #[test]
    fn first_segment_splits_on_the_delimiter() {
        assert_eq!(first_segment("org"), "org");
        assert_eq!(first_segment("org::team::x"), "org");
    }

    #[test]
    fn selected_or_on_path_covers_ancestors_and_descendants() {
        assert!(selected_or_on_path("org", "org::team::x"));
        assert!(selected_or_on_path("org::team::x", "org"));
        assert!(selected_or_on_path("org", "org"));
        assert!(!selected_or_on_path("foobar", "org::team::x"));
        assert!(!selected_or_on_path("org-standard", "org"));
    }

    #[tokio::test]
    async fn composes_qualified_ids_in_pre_order() {
        let fake = Fake::new()
            .meta(
                "org",
                vec![
                    git("team-a", "https://x/a.git", "main"),
                    git("team-b", "https://x/b.git", "main"),
                ],
            )
            .leaf("team-a")
            .leaf("team-b");
        let sources = vec![
            git("org", "https://x/org.git", "main"),
            git("solo", "https://x/solo.git", "main"),
        ];

        let out = expand_all(&fake, &sources).await.unwrap();
        let qids: Vec<&str> =
            out.iter().map(|e| e.qualified_id.as_str()).collect();
        assert_eq!(qids, vec!["org::team-a", "org::team-b", "solo"]);

        let ids: Vec<&str> = out.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["team-a", "team-b", "solo"]);
    }

    #[tokio::test]
    async fn detects_git_cycle_and_names_the_chain() {
        let fake = Fake::new()
            .meta("org", vec![git("org", "https://x/org.git", "main")]);
        let sources = vec![git("org", "https://x/org.git", "main")];

        let err = expand_all(&fake, &sources).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("cycle detected"), "{msg}");
        assert!(msg.contains("org → org::org"), "{msg}");
    }

    #[tokio::test]
    async fn detects_local_cycle() {
        let fake = Fake::new().meta("org", vec![local("org", "./same")]);
        let sources = vec![local("org", "./same")];

        let err = expand_all(&fake, &sources).await.unwrap_err();
        assert!(err.to_string().contains("cycle detected"));
    }

    #[tokio::test]
    async fn allows_a_diamond() {
        let fake = Fake::new()
            .meta(
                "org",
                vec![
                    git("team-a", "https://x/a.git", "main"),
                    git("team-b", "https://x/b.git", "main"),
                ],
            )
            .meta("team-a", vec![git("shared", "https://x/s.git", "main")])
            .meta("team-b", vec![git("shared", "https://x/s.git", "main")])
            .leaf("shared");
        let sources = vec![git("org", "https://x/org.git", "main")];

        let out = expand_all(&fake, &sources).await.unwrap();
        let qids: Vec<&str> =
            out.iter().map(|e| e.qualified_id.as_str()).collect();
        assert_eq!(qids, vec!["org::team-a::shared", "org::team-b::shared"]);
    }

    #[tokio::test]
    async fn both_node_contributes_its_own_leaf_and_recurses() {
        // `org` both contributes its own payload and composes a child. Its own
        // contribution is emitted in pre-order, before the child's.
        let fake = Fake::new()
            .both("org", vec![git("child", "https://x/c.git", "main")])
            .leaf("child");
        let sources = vec![git("org", "https://x/org.git", "main")];

        let out = expand_all(&fake, &sources).await.unwrap();
        let qids: Vec<&str> =
            out.iter().map(|e| e.qualified_id.as_str()).collect();
        assert_eq!(qids, vec!["org", "org::child"]);

        let payloads: Vec<&str> = out.iter().map(|e| e.leaf.as_str()).collect();
        assert_eq!(payloads, vec!["payload:org", "payload:child"]);
    }

    #[tokio::test]
    async fn selecting_a_child_prunes_a_both_nodes_own_leaf() {
        // `org` is on the path to the selection and is therefore traversed, but
        // its own payload is outside the selected subtree, so only the child's
        // contribution is emitted.
        let fake = Fake::new()
            .both("org", vec![git("child", "https://x/c.git", "main")])
            .leaf("child");
        let sources = vec![git("org", "https://x/org.git", "main")];

        let out = expand(
            &fake,
            &sources,
            &root(),
            DEFAULT_META_PROJECTION_DEPTH,
            Some("org::child"),
        )
        .await
        .unwrap();
        let qids: Vec<&str> =
            out.iter().map(|e| e.qualified_id.as_str()).collect();
        assert_eq!(qids, vec!["org::child"]);
    }

    #[tokio::test]
    async fn detects_a_cycle_through_a_both_node() {
        // A both-and node is pushed onto the ancestor path before recursing, so
        // a child that resolves back to it is still caught as a cycle.
        let fake = Fake::new()
            .both("org", vec![git("org", "https://x/org.git", "main")]);
        let sources = vec![git("org", "https://x/org.git", "main")];

        let err = expand_all(&fake, &sources).await.unwrap_err();
        assert!(err.to_string().contains("cycle detected"));
    }

    #[tokio::test]
    async fn depth_cap_aborts_a_too_deep_graph() {
        // org -> a -> b (two descents); a cap of 1 must abort.
        let fake = Fake::new()
            .meta("org", vec![git("a", "https://x/a.git", "main")])
            .meta("a", vec![git("b", "https://x/b.git", "main")])
            .leaf("b");
        let sources = vec![git("org", "https://x/org.git", "main")];

        let err = expand(&fake, &sources, &root(), 1, None).await.unwrap_err();
        assert!(err.to_string().contains("maximum depth"));

        let ok = expand(&fake, &sources, &root(), 2, None).await.unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].qualified_id, "org::a::b");
    }

    #[tokio::test]
    async fn selection_prunes_unrelated_subtrees() {
        let fake = Fake::new()
            .meta(
                "org",
                vec![
                    git("team-a", "https://x/a.git", "main"),
                    git("team-b", "https://x/b.git", "main"),
                ],
            )
            .leaf("team-a")
            .leaf("team-b");
        let sources = vec![
            git("org", "https://x/org.git", "main"),
            git("solo", "https://x/solo.git", "main"),
        ];

        let out = expand(
            &fake,
            &sources,
            &root(),
            DEFAULT_META_PROJECTION_DEPTH,
            Some("org::team-a"),
        )
        .await
        .unwrap();
        let qids: Vec<&str> =
            out.iter().map(|e| e.qualified_id.as_str()).collect();
        assert_eq!(qids, vec!["org::team-a"]);

        let classified = fake.classified.borrow().clone();
        assert!(classified.contains(&"org".to_string()));
        assert!(classified.contains(&"org::team-a".to_string()));
        assert!(!classified.contains(&"org::team-b".to_string()));
        assert!(!classified.contains(&"solo".to_string()));
    }

    #[tokio::test]
    async fn rejects_duplicate_sibling_ids() {
        let fake = Fake::new().meta(
            "org",
            vec![
                git("dupe", "https://x/a.git", "main"),
                git("dupe", "https://x/b.git", "main"),
            ],
        );
        let sources = vec![git("org", "https://x/org.git", "main")];

        let err = expand_all(&fake, &sources).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("duplicate member id 'dupe'"), "{msg}");
        assert!(msg.contains("bundle 'org'"), "{msg}");
    }

    #[tokio::test]
    async fn rejects_duplicate_top_level_ids() {
        let fake = Fake::new();
        let sources = vec![
            git("dupe", "https://x/a.git", "main"),
            git("dupe", "https://x/b.git", "main"),
        ];

        let err = expand_all(&fake, &sources).await.unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate projection source id 'dupe'")
        );
    }
}
