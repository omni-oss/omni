use cel::Program as CelProgram;
use derive_new::new;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::Context;

#[derive(Debug)]
pub struct Evaluator {
    program: CelProgram,
    source: String,
}

pub fn parse(expression: impl AsRef<str>) -> Result<Evaluator, Error> {
    let source = expression.as_ref();
    let program = CelProgram::compile(source)?;

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
                cel::ExecutionError::UndeclaredReference(name)
                    if is_invoked(&self.source, &name) =>
                {
                    ErrorInner::UnknownFunction {
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

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(pub(crate) ErrorInner);

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.0.discriminant()
    }

    pub fn class(&self) -> ErrorClass {
        use cel::ExecutionError as E;
        match &self.0 {
            ErrorInner::UnknownFunction { name } => {
                ErrorClass::MissingReference {
                    name: name.clone(),
                    is_call: true,
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

#[derive(Debug, thiserror::Error, EnumDiscriminants, new)]
#[strum_discriminants(name(ErrorKind), vis(pub))]
#[error("cel expression error: {inner}")]
pub enum ErrorInner {
    #[error(transparent)]
    ParseError(#[from] cel::ParseErrors),

    #[error(transparent)]
    ExecutionError(#[from] cel::ExecutionError),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),

    #[error("Undeclared reference to '{name}'")]
    UnknownFunction {
        #[new(into)]
        name: String,
    },

    #[error("filter must evaluate to a bool, got {got}")]
    NonBool {
        #[new(into)]
        got: String,
    },

    #[error("variable not found: {name}")]
    VariableNotFound {
        #[new(into)]
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
