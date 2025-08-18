pub trait Metadata {
    fn is_dir(&self) -> bool;
    fn is_file(&self) -> bool;
}
