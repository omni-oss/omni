pub trait Validator<T> {
    fn validate(&self, value: &T) -> Result<(), String>;
}

pub trait StaticValidator<T> {
    fn validate_static(value: &T) -> Result<(), String>;
}
