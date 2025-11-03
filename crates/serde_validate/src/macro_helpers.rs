pub macro impl_compound_static_validators($t:ident => $($v:ident),*$(,)?) {
    impl <$t, $($v: $crate::StaticValidator<$t>),*> $crate::StaticValidator<$t> for ($($v),*) {
        fn validate_static(value: &$t) -> Result<(), String> {
            $(
                <$v as $crate::StaticValidator<$t>>::validate_static(value)?;
            )*
            Ok(())
        }
    }
}

pub macro impl_compound_validators($t:ident => $($v:ident),*$(,)?) {
    impl <$t, $($v: $crate::Validator<$t>),*> $crate::Validator<$t> for ($($v),*) {

        #[allow(non_snake_case)]
        #[allow(nonstandard_style)]
        fn validate(&self, value: &$t) -> Result<(), String> {
            let ($($v,)*) = self;

            $(
                $v.validate(value)?;
            )*

            Ok(())
        }
    }
}
