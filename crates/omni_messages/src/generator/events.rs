use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorStartEvent {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorFileCreatedEvent {
    pub generator: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorFileSkippedEvent {
    pub generator: String,
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorCompletedEvent {
    pub name: String,
}
