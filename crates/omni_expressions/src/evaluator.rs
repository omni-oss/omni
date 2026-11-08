use cel::Program as CelProgram;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::Context;

omni_codes::prefixes!("expression");

#[derive(Debug)]
pub struct Evaluator {
    program: CelProgram,
    source: String,
}

pub fn parse(expression: impl AsRef<str>) -> Result<Evaluator, Error> {
    let source = expression.as_ref();
    let program =
        CelProgram::compile(source).map_err(ErrorInner::ParseError)?;

    Ok(Evaluator {
        program,
        source: source.to_string(),
    })
}

impl Evaluator {
    /// Evaluates the expression, which must produce a boolean. Runtime errors
    /// and non-boolean results (including `null`) are returned as errors; no
    /// truthiness coercion is applied.
    pub fn evaluate_bool<'p, 'v>(
        &self,
        context: &Context<'p, 'v>,
    ) -> Result<bool, Error> {
        let result =
            self.program.execute(&context.inner).map_err(|e| match e {
                cel::ExecutionError::UndeclaredReference(name) => {
                    ErrorInner::UndeclaredReference {
                        is_call: is_invoked(&self.source, &name),
                        name: name.to_string(),
                    }
                }
                e => ErrorInner::ExecutionError(e),
            })?;

        match result {
            cel::Value::Bool(b) => Ok(b),
            other => Err(ErrorInner::NonBool {
                got: other.type_of().to_string(),
            }
            .into()),
        }
    }
}

fn is_invoked(source: &str, name: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let code = strip_string_literals(source);
    code.match_indices(name).any(|(i, _)| {
        let before_ok =
            code[..i].chars().next_back().is_none_or(|c| !is_ident(c));
        let after = code[i + name.len()..].trim_start();
        before_ok && after.starts_with('(')
    })
}

/// Blanks out the contents of quoted string literals so call-like text inside
/// them is never mistaken for a call.
fn strip_string_literals(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in source.chars() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                    out.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

/// The summary names the position only: cel's own message quotes the
/// expression text, which can hold secrets.
fn parse_location(errors: &cel::ParseErrors) -> String {
    match errors.errors.first() {
        Some(e) => format!(" at line {}, column {}", e.pos.0, e.pos.1),
        None => String::new(),
    }
}

/// Structured classification of an evaluation error, so callers never need to
/// match on `cel` types or message text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorClass {
    MissingReference { name: String, is_call: bool },
    MissingKey { key: String },
    TypeMismatch { detail: String },
    NonBool { got: String },
    Other,
}

#[derive(Debug, thiserror::Error, omni_diagnostic::Diagnostic)]
#[error(transparent)]
#[diagnostic(forward)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }

    pub fn class(&self) -> ErrorClass {
        use cel::ExecutionError as E;
        match &self.0 {
            ErrorInner::UndeclaredReference { name, is_call } => {
                ErrorClass::MissingReference {
                    name: name.clone(),
                    is_call: *is_call,
                }
            }
            ErrorInner::NonBool { got } => {
                ErrorClass::NonBool { got: got.clone() }
            }
            ErrorInner::ExecutionError(e) => match e {
                E::UndeclaredReference(name) => ErrorClass::MissingReference {
                    name: name.to_string(),
                    is_call: false,
                },
                E::NoSuchKey(key) => ErrorClass::MissingKey {
                    key: key.to_string(),
                },
                E::NoSuchOverload(_)
                | E::UnexpectedType { .. }
                | E::ValuesNotComparable(_, _)
                | E::UnsupportedBinaryOperator(_, _, _) => {
                    ErrorClass::TypeMismatch {
                        detail: e.to_string(),
                    }
                }
                _ => ErrorClass::Other,
            },
            _ => ErrorClass::Other,
        }
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        Self(inner)
    }
}

#[derive(
    Debug, thiserror::Error, EnumDiscriminants, new, omni_diagnostic::Diagnostic,
)]
#[strum_discriminants(name(ErrorKind), vis(pub))]
#[error("cel expression error: {inner}")]
#[allow(clippy::redundant_field_names)]
pub enum ErrorInner {
    /// The expression does not parse or compile.
    #[error("invalid expression{}", parse_location(.0))]
    #[diagnostic(
        code = "expression.invalid",
        hint = "check the expression syntax: quotes, brackets, operators and keywords"
    )]
    ParseError(cel::ParseErrors),

    #[error(transparent)]
    #[diagnostic(skip)]
    ExecutionError(#[from] cel::ExecutionError),

    #[error(transparent)]
    #[diagnostic(delegate)]
    Unknown(#[from] eyre::Report),

    /// The expression refers to a name or function that is not defined.
    #[error("Undeclared reference to '{name}'")]
    #[diagnostic(
        code = "expression.undeclared_reference",
        hint = "check the spelling of `{name}`; it must be a defined variable or function"
    )]
    UndeclaredReference {
        #[new(into)]
        #[diagnostic(detail)]
        name: String,
        is_call: bool,
    },

    /// The expression evaluated to a value that is not a boolean.
    #[error("filter must evaluate to a bool, got {got}")]
    #[diagnostic(
        code = "expression.not_boolean",
        hint = "make the expression evaluate to `true` or `false`"
    )]
    NonBool {
        #[new(into)]
        #[diagnostic(detail)]
        got: String,
    },

    /// The expression names a variable that is not in its context.
    #[error("variable not found: {name}")]
    #[diagnostic(
        code = "expression.variable_not_found",
        hint = "define `{name}` in the expression context"
    )]
    VariableNotFound {
        #[new(into)]
        #[diagnostic(detail)]
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! map {
        ($($key:expr => $value:expr),*$(,)?) => {{
            let mut map = std::collections::HashMap::new();
            $(
                map.insert($key, $value);
            )*
            map
        }};
    }

    #[test]
    fn test_simple_expression() {
        let evaluator = parse("1 + 1 == 2").unwrap();
        assert!(
            evaluator
                .evaluate_bool(&Default::default())
                .expect("should be true")
        );
    }

    #[test]
    fn test_simple_expression_with_simple_variables() {
        let evaluator = parse(
            "a == 1 && b == \"test\" && b.startsWith(\"tes\") && 1 in c && d.a == 1",
        )
        .unwrap();
        let mut context = Context::default();
        context
            .add_variable("a", 1)
            .expect("Failed to add variable");

        context
            .add_variable("b", "test")
            .expect("Failed to add variable");

        context
            .add_variable("c", [1, 2, 3])
            .expect("Failed to add variable");

        let m = map! {
            "a" => 1,
            "b" => 2,
            "c" => 3,
        };

        context
            .add_variable("d", &m)
            .expect("Failed to add variable");

        assert!(
            evaluator
                .evaluate_bool(&context)
                .expect("Failed to evaluate"),
            "Expression should be true"
        );
    }

    fn eval(expr: &str, ctx: &Context) -> Result<bool, Error> {
        parse(expr).unwrap().evaluate_bool(ctx)
    }

    #[test]
    fn non_bool_results_are_errors() {
        let ctx = Context::default();
        for (expr, got) in [
            ("1", "int"),
            ("\"x\"", "string"),
            ("[]", "list"),
            ("{}", "map"),
            ("null", "null"),
        ] {
            let e = eval(expr, &ctx).unwrap_err();
            assert_eq!(
                e.class(),
                ErrorClass::NonBool { got: got.into() },
                "{expr}"
            );
        }
    }

    #[test]
    fn runtime_errors_are_errors() {
        let ctx = Context::default();
        assert!(eval("1 + \"a\" == 1", &ctx).is_err());
        assert!(eval("1 / 0 == 1", &ctx).is_err());
        assert!(matches!(
            eval("knd == \"e2e\"", &ctx).unwrap_err().class(),
            ErrorClass::MissingReference { is_call: false, .. }
        ));
        assert!(matches!(
            eval("\"a\".startWith(\"a\")", &ctx).unwrap_err().class(),
            ErrorClass::MissingReference { is_call: true, .. }
        ));
    }

    #[test]
    fn missing_key_and_has() {
        let mut ctx = Context::default();
        let m = map! { "a" => 1 };
        ctx.add_variable("meta", &m).unwrap();
        assert!(matches!(
            eval("meta.b == 1", &ctx).unwrap_err().class(),
            ErrorClass::MissingKey { .. }
        ));
        assert!(!eval("has(meta.b)", &ctx).unwrap());
        assert!(eval("has(meta.a)", &ctx).unwrap());
    }

    #[test]
    fn type_mismatch() {
        let mut ctx = Context::default();
        ctx.add_variable("p", 1).unwrap();
        assert!(matches!(
            eval("p > \"high\"", &ctx).unwrap_err().class(),
            ErrorClass::TypeMismatch { .. }
        ));
    }

    #[test]
    fn call_text_in_string_literal_is_not_a_call() {
        let ctx = Context::default();
        let e = eval("foo == true || \"foo(\" == \"bar\"", &ctx).unwrap_err();
        assert!(matches!(
            e.class(),
            ErrorClass::MissingReference { is_call: false, .. }
        ));
    }
}
