use std::fmt::Write;

use crate::{
    contributor::Contribution,
    fence::{FENCE_BEGIN, FENCE_END_PREFIX},
};

/// Concatenate contributions in the given order, keeping each contributor's own
/// order. There is no global sort: negations are order-sensitive. The end fence
/// carries a short content hash of the rendered pattern lines.
pub fn render_block(contributions: &[Contribution]) -> String {
    let mut body_lines: Vec<&str> = Vec::new();
    for contribution in contributions {
        for pattern in &contribution.patterns {
            body_lines.push(pattern.as_str());
        }
    }
    let body = body_lines.join("\n");
    let hash = short_hash(body.as_bytes());

    if body.is_empty() {
        format!("{FENCE_BEGIN}\n{FENCE_END_PREFIX} {hash}")
    } else {
        format!("{FENCE_BEGIN}\n{body}\n{FENCE_END_PREFIX} {hash}")
    }
}

fn short_hash(bytes: &[u8]) -> String {
    let hash = omni_hasher::blake3::hash_bytes(bytes)
        .expect("blake3 hashing of an in-memory buffer never fails");
    let mut hex = String::with_capacity(8);
    for byte in &hash[..4] {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        IgnoreContributor, MockIgnoreContributor, pattern::IgnorePattern,
    };

    fn contribution(name: &'static str, lines: &[&str]) -> Contribution {
        Contribution {
            name,
            patterns: lines
                .iter()
                .map(|l| IgnorePattern::raw(l).unwrap())
                .collect(),
        }
    }

    #[test]
    fn contributor_order_is_preserved_with_no_global_sort() {
        let internal = contribution(
            "internal",
            &["/.omni/sources/*/**", "!/.omni/sources/*/lock.json"],
        );
        let projections = contribution("projections", &["/.agents/skills/z"]);
        let block = render_block(&[internal, projections]);

        let body: Vec<&str> = block
            .lines()
            .filter(|l| !l.starts_with("# @@omni-managed"))
            .collect();
        assert_eq!(
            body,
            vec![
                "/.omni/sources/*/**",
                "!/.omni/sources/*/lock.json",
                "/.agents/skills/z",
            ]
        );
    }

    #[test]
    fn identical_inputs_render_byte_identical_blocks() {
        let a = render_block(&[contribution("x", &["/a", "/b"])]);
        let b = render_block(&[contribution("x", &["/a", "/b"])]);
        assert_eq!(a, b);
    }

    #[test]
    fn block_is_wrapped_in_fences_with_a_hash() {
        let block = render_block(&[contribution("x", &["/a"])]);
        let mut lines = block.lines();
        assert_eq!(lines.next().unwrap(), FENCE_BEGIN);
        assert_eq!(lines.next().unwrap(), "/a");
        let end = lines.next().unwrap();
        assert!(end.starts_with(FENCE_END_PREFIX));
        assert_eq!(end.len(), FENCE_END_PREFIX.len() + 1 + 8);
    }

    #[tokio::test]
    async fn a_contributor_feeds_its_patterns_into_a_contribution() {
        let mut mock = MockIgnoreContributor::new();
        mock.expect_name().return_const("projections");
        mock.expect_patterns().returning(|| {
            Ok(vec![IgnorePattern::path(std::path::Path::new("x"))])
        });

        let patterns = mock.patterns().await.unwrap();
        let block = render_block(&[Contribution {
            name: mock.name(),
            patterns,
        }]);
        assert!(block.contains("/x"));
    }
}
