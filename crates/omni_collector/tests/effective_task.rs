//! Behavior and golden vectors for the effective-task-config digest (F06.2).

use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

use omni_collector::effective_task::{
    EffectiveTaskConfig, EffectiveValue, Enabled, InputPolicy, OutputPolicy,
    PathRule, VERSION,
};
use omni_command_config::{CommandConfig, CommandOperand};
use omni_hasher::{Hasher, impls::DefaultHasher};
use omni_retry_config::{Backoff, Jitter, RetryPolicy};
use omni_types::Root;

/// One named edit to a fully populated config.
type Mutation = Box<dyn Fn(&mut EffectiveTaskConfig)>;

fn cfg() -> EffectiveTaskConfig {
    EffectiveTaskConfig::default()
}

fn rule(root: Root, pattern: &str) -> PathRule {
    PathRule {
        root,
        pattern: pattern.to_string(),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest_hex(c: &EffectiveTaskConfig) -> String {
    hex(&c.digest())
}

fn assert_differs(
    name: &str,
    a: &EffectiveTaskConfig,
    b: &EffectiveTaskConfig,
) {
    assert_ne!(a.encode(), b.encode(), "{name}: encodings are equal");
    assert_ne!(a.digest(), b.digest(), "{name}: digests are equal");
}

fn assert_same(name: &str, a: &EffectiveTaskConfig, b: &EffectiveTaskConfig) {
    assert_eq!(a.encode(), b.encode(), "{name}: encodings differ");
    assert_eq!(a.digest(), b.digest(), "{name}: digests differ");
}

/// An independent writer of the layout documented on the module, so the encoder
/// is checked against the spec and not against itself.
#[derive(Default)]
struct Spec(Vec<u8>);

impl Spec {
    fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }
    fn u64(mut self, v: u64) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    fn str(self, s: &str) -> Self {
        let mut me = self.u64(s.len() as u64);
        me.0.extend(s.as_bytes());
        me
    }
    fn duration(self, secs: u64) -> Self {
        self.u64(secs).u32(0)
    }
}

#[test]
fn the_default_config_encodes_exactly_as_documented() {
    let expected = Spec::default()
        .str("effective-task-config/v1")
        .u8(1) // command
        .u8(0) // None
        .u8(2) // retry_command
        .u8(0)
        .u8(3) // enabled
        .u8(0) // Bool
        .u8(1) // true
        .u8(4) // interactive
        .u8(0)
        .u8(5) // persistent
        .u8(0)
        .u8(6) // retry: max_retries
        .u8(0)
        .u8(2) // Exponential
        .u64(2.0_f64.to_bits())
        .duration(1) // base
        .duration(60) // max_delay
        .u8(0) // jitter None
        .u8(7) // args
        .u64(0)
        .u8(8) // env_workspace
        .u64(0)
        .u8(14) // env_project
        .u64(0)
        .u8(15) // env_task
        .u64(0)
        .u8(9) // meta
        .u64(0)
        .u8(10) // dependencies
        .u64(0)
        .u8(11) // siblings
        .u64(0)
        .u8(12) // input: key_defaults
        .u8(0)
        .u64(0) // include
        .u64(0) // exclude
        .u64(0) // env_keys
        .u8(13) // output: include
        .u64(0)
        .u64(0) // exclude
        .u8(0) // capture_logs
        .0;
    assert_eq!(hex(&cfg().encode()), hex(&expected));
    assert_eq!(VERSION, "effective-task-config/v1");
}

#[test]
fn the_digest_is_the_content_hasher_over_the_encoding() {
    let c = rich();
    assert_eq!(c.digest(), DefaultHasher::hash(&c.encode()));
    assert_eq!(c.digest(), c.clone().digest(), "not deterministic");
}

fn rich() -> EffectiveTaskConfig {
    EffectiveTaskConfig {
        command: Some(CommandConfig::operands([
            CommandOperand::literal("cargo"),
            CommandOperand::expression("{{ meta.profile }}"),
            CommandOperand::interpolated("{{ n }}", [("n", "v")]),
        ])),
        retry_command: Some(CommandConfig::argv(["echo", "retry"])),
        enabled: Enabled::Expression("{{ meta.on }}".into()),
        interactive: true,
        persistent: false,
        retry: RetryPolicy {
            max_retries: 3,
            backoff: Backoff::Linear {
                step: Duration::from_millis(1500),
            },
            base: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter: Jitter::Full,
        },
        args: [
            ("count".to_string(), EffectiveValue::Int(-7)),
            ("ratio".to_string(), EffectiveValue::Float(0.5)),
            (
                "list".to_string(),
                EffectiveValue::List(vec![
                    EffectiveValue::Null,
                    EffectiveValue::Bool(true),
                    EffectiveValue::Uint(u64::MAX),
                    EffectiveValue::String("x".into()),
                ]),
            ),
        ]
        .into(),
        env_workspace: [("W".to_string(), "1".to_string())].into(),
        env_project: [("A".to_string(), "1".to_string())].into(),
        env_task: [("T".to_string(), "1".to_string())].into(),
        meta: [(
            "nested".to_string(),
            EffectiveValue::Map(
                [("k".to_string(), EffectiveValue::String("v".into()))].into(),
            ),
        )]
        .into(),
        dependencies: ["app#build".to_string(), "lib#build".to_string()].into(),
        siblings: ["app#lint".to_string()].into(),
        input: InputPolicy {
            key_defaults: true,
            include: vec![rule(Root::Project, "src/**")],
            exclude: vec![rule(Root::Workspace, "**/*.md")],
            env_keys: ["CI".to_string()].into(),
        },
        output: OutputPolicy {
            include: vec![rule(Root::Project, "dist/**")],
            exclude: vec![],
            capture_logs: true,
        },
    }
}

/// Golden vectors: if this fails, the encoding changed. That is only legitimate
/// with a new `VERSION`, because every existing key would silently change.
#[test]
fn golden_vector_for_a_fully_populated_config() {
    const ENCODING: &str = "18000000000000006566666563746976652d7461736b2d636f6e6669672f76310101020300000000000000000500000000000000636172676f0112000000000000007b7b206d6574612e70726f66696c65207d7d0207000000000000007b7b206e207d7d010000000000000001000000000000006e010000000000000076020101020000000000000004000000000000006563686f0500000000000000726574727903010d000000000000007b7b206d6574612e6f6e207d7d0401050006030101000000000000000065cd1d00000000000000000065cd1d1e0000000000000000000000010703000000000000000500000000000000636f756e7402f9ffffffffffffff04000000000000006c69737406040000000000000000010103ffffffffffffffff050100000000000000780500000000000000726174696f04000000000000e03f0801000000000000000100000000000000570100000000000000310e01000000000000000100000000000000410100000000000000310f010000000000000001000000000000005401000000000000003109010000000000000006000000000000006e657374656407010000000000000001000000000000006b050100000000000000760a02000000000000000900000000000000617070236275696c6409000000000000006c6962236275696c640b01000000000000000800000000000000617070236c696e740c0101000000000000000106000000000000007372632f2a2a01000000000000000007000000000000002a2a2f2a2e6d640100000000000000020000000000000043490d0100000000000000010700000000000000646973742f2a2a000000000000000001";
    const DIGEST: &str =
        "953a5c430df37360b07b10a5cc469fda72458de1fcb23f8a2880464bddeae72a";
    let c = rich();
    assert_eq!(hex(&c.encode()), ENCODING, "the canonical encoding changed");
    assert_eq!(digest_hex(&c), DIGEST, "the digest changed");
}

#[test]
fn every_included_scalar_field_changes_the_digest() {
    let base = rich();
    let mutations: Vec<(&str, Mutation)> = vec![
        ("command", Box::new(|c| c.command = None)),
        ("retry_command", Box::new(|c| c.retry_command = None)),
        (
            "enabled bool",
            Box::new(|c| c.enabled = Enabled::Bool(true)),
        ),
        (
            "enabled expr text",
            Box::new(|c| {
                c.enabled = Enabled::Expression("{{ meta.x }}".into())
            }),
        ),
        ("interactive", Box::new(|c| c.interactive = false)),
        ("persistent", Box::new(|c| c.persistent = true)),
        ("retry max_retries", Box::new(|c| c.retry.max_retries = 4)),
        (
            "retry backoff kind",
            Box::new(|c| c.retry.backoff = Backoff::Fixed),
        ),
        (
            "retry backoff step",
            Box::new(|c| {
                c.retry.backoff = Backoff::Linear {
                    step: Duration::from_secs(2),
                }
            }),
        ),
        (
            "retry base",
            Box::new(|c| c.retry.base = Duration::from_millis(501)),
        ),
        (
            "retry max_delay",
            Box::new(|c| c.retry.max_delay = Duration::from_secs(31)),
        ),
        ("retry jitter", Box::new(|c| c.retry.jitter = Jitter::Equal)),
        ("args", Box::new(|c| c.args.clear())),
        ("env workspace", Box::new(|c| c.env_workspace.clear())),
        ("env project", Box::new(|c| c.env_project.clear())),
        ("env task", Box::new(|c| c.env_task.clear())),
        ("meta", Box::new(|c| c.meta.clear())),
        ("dependencies", Box::new(|c| c.dependencies.clear())),
        ("siblings", Box::new(|c| c.siblings.clear())),
        (
            "input key_defaults",
            Box::new(|c| c.input.key_defaults = false),
        ),
        ("input include", Box::new(|c| c.input.include.clear())),
        ("input exclude", Box::new(|c| c.input.exclude.clear())),
        ("input env_keys", Box::new(|c| c.input.env_keys.clear())),
        ("output include", Box::new(|c| c.output.include.clear())),
        (
            "output exclude",
            Box::new(|c| c.output.exclude.push(rule(Root::Project, "x"))),
        ),
        (
            "output capture_logs",
            Box::new(|c| c.output.capture_logs = false),
        ),
    ];
    for (name, mutate) in mutations {
        let mut changed = base.clone();
        mutate(&mut changed);
        assert_differs(name, &base, &changed);
    }
}

#[test]
fn an_explicit_default_equals_leaving_it_out() {
    let explicit = EffectiveTaskConfig {
        enabled: Enabled::Bool(true),
        retry: RetryPolicy {
            max_retries: 0,
            backoff: Backoff::Exponential { factor: 2.0 },
            base: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            jitter: Jitter::None,
        },
        ..cfg()
    };
    assert_same("explicit defaults", &cfg(), &explicit);
}

#[test]
fn a_boolean_and_an_expression_are_different_definitions() {
    let a = EffectiveTaskConfig {
        enabled: Enabled::Bool(true),
        ..cfg()
    };
    let b = EffectiveTaskConfig {
        enabled: Enabled::Expression("true".into()),
        ..cfg()
    };
    assert_differs("bool vs expression", &a, &b);
}

#[test]
fn command_variants_with_the_same_text_never_collide() {
    let with = |command| EffectiveTaskConfig {
        command: Some(command),
        ..cfg()
    };
    let shell = with(CommandConfig::shell("$VAR"));
    let argv = with(CommandConfig::argv(["$VAR"]));
    let literal =
        with(CommandConfig::operands([CommandOperand::literal("$VAR")]));
    let expr = with(CommandConfig::operands([CommandOperand::expression(
        "$VAR",
    )]));
    let all = [&shell, &argv, &literal, &expr];
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.digest(), b.digest());
        }
    }
}

#[test]
fn interpolated_bindings_and_single_pass_substitution_are_distinguished() {
    let with = |op| EffectiveTaskConfig {
        command: Some(CommandConfig::operands([op])),
        ..cfg()
    };
    let a = with(CommandOperand::interpolated("{{ n }}", [("n", "v1")]));
    let b = with(CommandOperand::interpolated("{{ n }}", [("n", "v2")]));
    let c = with(CommandOperand::interpolated("{{ n }}", [("m", "v1")]));
    let plain = with(CommandOperand::expression("{{ n }}"));
    assert_differs("binding value", &a, &b);
    assert_differs("binding name", &a, &c);
    assert_differs("interpolated vs plain expression", &a, &plain);
}

#[test]
fn main_and_retry_commands_are_not_interchangeable() {
    let one = CommandConfig::shell("a");
    let two = CommandConfig::shell("b");
    let a = EffectiveTaskConfig {
        command: Some(one.clone()),
        retry_command: Some(two.clone()),
        ..cfg()
    };
    let b = EffectiveTaskConfig {
        command: Some(two),
        retry_command: Some(one),
        ..cfg()
    };
    assert_differs("command swap", &a, &b);
}

#[test]
fn list_order_is_semantic_but_a_boundary_shift_is_not_a_collision() {
    let argv = |items: &[&str]| EffectiveTaskConfig {
        command: Some(CommandConfig::argv(items.iter().copied())),
        ..cfg()
    };
    assert_differs("order", &argv(&["a", "b"]), &argv(&["b", "a"]));
    assert_differs("boundary", &argv(&["ab", "c"]), &argv(&["a", "bc"]));
    assert_differs("split", &argv(&["ab"]), &argv(&["a", "b"]));
    assert_differs("empty element", &argv(&["a", ""]), &argv(&["a"]));

    let ops = |items: Vec<CommandOperand>| EffectiveTaskConfig {
        command: Some(CommandConfig::operands(items)),
        ..cfg()
    };
    let x = || CommandOperand::literal("x");
    let y = || CommandOperand::expression("y");
    assert_differs("operand order", &ops(vec![x(), y()]), &ops(vec![y(), x()]));
}

#[test]
fn presence_and_type_are_part_of_the_encoding() {
    let with_cmd = |command| EffectiveTaskConfig { command, ..cfg() };
    assert_differs(
        "none vs empty shell",
        &with_cmd(None),
        &with_cmd(Some(CommandConfig::shell(""))),
    );
    assert_differs(
        "empty argv vs empty operands",
        &with_cmd(Some(CommandConfig::argv(Vec::<String>::new()))),
        &with_cmd(Some(CommandConfig::operands([]))),
    );

    let arg = |v| EffectiveTaskConfig {
        args: [("a".to_string(), v)].into(),
        ..cfg()
    };
    assert_differs(
        "string vs int",
        &arg(EffectiveValue::String("1".into())),
        &arg(EffectiveValue::Int(1)),
    );
    assert_differs(
        "int vs float",
        &arg(EffectiveValue::Int(1)),
        &arg(EffectiveValue::Float(1.0)),
    );
    assert_differs(
        "null vs false",
        &arg(EffectiveValue::Null),
        &arg(EffectiveValue::Bool(false)),
    );
    assert_differs(
        "empty list vs empty map",
        &arg(EffectiveValue::List(vec![])),
        &arg(EffectiveValue::Map(BTreeMap::new())),
    );
}

#[test]
fn equal_content_in_different_fields_does_not_collide() {
    let value = || [("a".to_string(), EffectiveValue::Int(1))].into();
    let as_args = EffectiveTaskConfig {
        args: value(),
        ..cfg()
    };
    let as_meta = EffectiveTaskConfig {
        meta: value(),
        ..cfg()
    };
    assert_differs("args vs meta", &as_args, &as_meta);

    let names = || -> BTreeSet<String> { ["app#build".to_string()].into() };
    let dep = EffectiveTaskConfig {
        dependencies: names(),
        ..cfg()
    };
    let sibling = EffectiveTaskConfig {
        siblings: names(),
        ..cfg()
    };
    assert_differs("dependency vs co-scheduled", &dep, &sibling);
}

#[test]
fn maps_are_canonical_whatever_order_they_were_built_in() {
    let from_json = |json: &str| -> EffectiveValue {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .into()
    };
    let one = from_json(r#"{"b":1,"a":{"y":2,"x":[3,4]}}"#);
    let two = from_json(r#"{"a":{"x":[3,4],"y":2},"b":1}"#);
    let with = |v: EffectiveValue| EffectiveTaskConfig {
        meta: [("m".to_string(), v)].into(),
        ..cfg()
    };
    assert_same("json key order", &with(one), &with(two));

    // A list inside a map keeps its own order.
    let reordered = from_json(r#"{"a":{"x":[4,3],"y":2},"b":1}"#);
    assert_differs(
        "list order inside a map",
        &with(from_json(r#"{"b":1,"a":{"y":2,"x":[3,4]}}"#)),
        &with(reordered),
    );
}

#[test]
fn unordered_sets_ignore_order_and_repetition_but_not_content() {
    let rules = |include: Vec<PathRule>| EffectiveTaskConfig {
        input: InputPolicy {
            include,
            ..Default::default()
        },
        ..cfg()
    };
    let a = rule(Root::Project, "src/**");
    let b = rule(Root::Workspace, "docs/**");
    assert_same(
        "order and duplicates",
        &rules(vec![a.clone(), b.clone()]),
        &rules(vec![b.clone(), a.clone(), a.clone()]),
    );
    assert_differs(
        "root",
        &rules(vec![rule(Root::Project, "src/**")]),
        &rules(vec![rule(Root::Workspace, "src/**")]),
    );

    let include_only = EffectiveTaskConfig {
        input: InputPolicy {
            include: vec![a.clone()],
            ..Default::default()
        },
        ..cfg()
    };
    let exclude_only = EffectiveTaskConfig {
        input: InputPolicy {
            exclude: vec![a],
            ..Default::default()
        },
        ..cfg()
    };
    assert_differs("include vs exclude", &include_only, &exclude_only);

    let deps = |names: &[&str]| EffectiveTaskConfig {
        dependencies: names.iter().map(|n| n.to_string()).collect(),
        ..cfg()
    };
    assert_same("dependency order", &deps(&["a", "b"]), &deps(&["b", "a"]));
}

#[test]
fn floats_are_canonical_and_json_numbers_keep_their_type() {
    let f = |v| EffectiveTaskConfig {
        args: [("f".to_string(), EffectiveValue::Float(v))].into(),
        ..cfg()
    };
    assert_same("negative zero", &f(0.0), &f(-0.0));
    assert_same(
        "nan",
        &f(f64::NAN),
        &f(f64::from_bits(0x7ff8_0000_dead_beef)),
    );
    assert_differs("distinct floats", &f(0.1), &f(0.2));

    let num = |json: &str| -> EffectiveValue {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .into()
    };
    assert_eq!(num("3"), EffectiveValue::Int(3));
    assert_eq!(num("3.0"), EffectiveValue::Float(3.0));
    assert_eq!(num("-1"), EffectiveValue::Int(-1));
    assert_eq!(num("18446744073709551615"), EffectiveValue::Uint(u64::MAX));
    assert_eq!(num("9223372036854775807"), EffectiveValue::Int(i64::MAX));
}

#[test]
fn the_version_tag_separates_encodings() {
    let c = cfg();
    let bytes = c.encode();
    let tag = VERSION.as_bytes();
    assert_eq!(&bytes[..8], &(tag.len() as u64).to_le_bytes());
    assert_eq!(&bytes[8..8 + tag.len()], tag);
}

#[test]
fn unrelated_task_changes_cannot_reach_this_digest() {
    // The type has no field for another task's definition, a description,
    // provenance, display policy or results, so editing those cannot change the
    // digest. Naming a sibling does: that is its co-scheduling relationship.
    let base = rich();
    let mut renamed = base.clone();
    renamed.siblings = ["app#lint".to_string(), "app#fmt".to_string()].into();
    assert_differs("a new co-scheduled task", &base, &renamed);
    assert_same("identical effective semantics", &base, &base.clone());
}
