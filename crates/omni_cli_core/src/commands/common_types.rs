use clap::ValueEnum;

#[derive(
    ValueEnum, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
)]
pub enum SerializationFormat {
    Json,
    Yaml,
    Toml,
}

impl SerializationFormat {
    pub fn to_serde_format(&self) -> omni_file_data_serde::Format {
        match self {
            SerializationFormat::Yaml => omni_file_data_serde::Format::Yaml,
            SerializationFormat::Json => omni_file_data_serde::Format::Json,
            SerializationFormat::Toml => omni_file_data_serde::Format::Toml,
        }
    }
}
