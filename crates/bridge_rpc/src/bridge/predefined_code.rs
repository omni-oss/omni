#[macro_export]
macro_rules! predefined_codes {
    (
        $ty:ty {
            $(
                $name:ident = $value:expr
            );
            *$(;)?
        }
    ) => {
        impl $ty {
            $(
                pub const $name: $ty = <$ty>::new($value);
            )*
        }
    };
}
