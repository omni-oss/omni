use clap::{ValueEnum, builder::PossibleValue};
use strum::VariantArray;

pub trait EnumValueAdapterContract:
    ToString + Clone + Copy + VariantArray + 'static
{
}

impl<T: ToString + Clone + Copy + VariantArray + 'static>
    EnumValueAdapterContract for T
{
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(transparent)]
pub struct EnumValueAdapter<T: EnumValueAdapterContract>(T);

impl<T: EnumValueAdapterContract> EnumValueAdapter<T> {
    #[inline(always)]
    fn variants() -> &'static [Self] {
        // SAFETY: `EnumValueAdapter<T>` is #[repr(transparent)] over `T`,
        // so the memory layout of `[T]` and `[EnumValueAdapter<T>]` is identical.
        unsafe { std::mem::transmute(T::VARIANTS) }
    }

    pub const fn new(value: T) -> Self {
        Self(value)
    }

    pub const fn value(&self) -> T {
        self.0
    }
}

impl<T: EnumValueAdapterContract> ValueEnum for EnumValueAdapter<T> {
    fn value_variants<'a>() -> &'a [Self] {
        Self::variants()
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        let text = self.0.to_string();
        Some(PossibleValue::new(text))
    }
}

impl<T: EnumValueAdapterContract> ToString for EnumValueAdapter<T> {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}
