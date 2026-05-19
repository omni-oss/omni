use clap::ValueEnum;

#[derive(
    ValueEnum, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum SerializationFormat {
    Json,
    Yaml,
    Toml,
}
