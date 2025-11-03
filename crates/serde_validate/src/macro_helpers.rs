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
