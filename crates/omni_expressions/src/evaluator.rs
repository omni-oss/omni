use cel::Program as CelProgram;
use strum::{EnumDiscriminants, IntoDiscriminant as _};

use crate::Context;

#[derive(Debug)]
pub struct Evaluator(CelProgram);

pub fn parse(expression: impl AsRef<str>) -> Result<Evaluator, Error> {
    let program = CelProgram::compile(expression.as_ref())?;

    Ok(Evaluator(program))
}

impl Evaluator {
    /// Evaluates the expression to and coerces it to a boolean including errors.
    pub fn coerce_to_bool<'a>(
        &self,
        context: &Context<'a>,
    ) -> Result<bool, Error> {
        let context = &context.inner;
        let result = self.0.execute(context)?;

        Ok(match result {
            cel::Value::List(_)
            | cel::Value::Map(_)
            | cel::Value::Function(_, _)
            | cel::Value::Bytes(_) => true,
            cel::Value::Int(i) => i != 0,
            cel::Value::UInt(i) => i != 0,
            cel::Value::Float(f) => f != 0.0,
            cel::Value::String(s) => !s.is_empty(),
            cel::Value::Bool(b) => b,
            cel::Value::Duration(time_delta) => !time_delta.is_zero(),
            cel::Value::Timestamp(date_time) => {
                date_time.timestamp_millis() != 0
            }
            cel::Value::Null => false,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("cel expression error: {inner:?}")]
pub struct Error {
    inner: ErrorInner,
    kind: ErrorKind,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl<T: Into<ErrorInner>> From<T> for Error {
    fn from(inner: T) -> Self {
        let inner = inner.into();
        let kind = inner.discriminant();
        Self { inner, kind }
    }
}

#[derive(Debug, thiserror::Error, EnumDiscriminants)]
#[strum_discriminants(name(ErrorKind), vis(pub))]
#[error("cel expression error: {inner}")]
pub enum ErrorInner {
    #[error(transparent)]
    ParseError(#[from] cel::ParseErrors),

    #[error(transparent)]
    ExecutionError(#[from] cel::ExecutionError),

    #[error(transparent)]
    Unknown(#[from] eyre::Report),
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
                .coerce_to_bool(&Default::default())
                .expect("should be true")
        );
    }

    #[test]
    fn test_simple_expression_with_simple_variables() {
        let evaluator = parse(
            "a == 1 && b == \"test\" && b.startsWith(\"tes\") && c.contains(1) && d.a == 1",
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
            .add_variable("c", &[1, 2, 3])
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
                .coerce_to_bool(&context)
                .expect("Failed to evaluate"),
            "Expression should be true"
        );
    }
}
