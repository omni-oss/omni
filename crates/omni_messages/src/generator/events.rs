use serde::{Deserialize, Serialize};

/// An invalid answer to an interactive generator prompt, before re-prompting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorPromptRejectedEvent {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorStartEvent {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorActionSkippedEvent {
    pub name: String,
    pub reason: Option<String>,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorActionInProgressEvent {
    pub name: String,
    pub message: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorActionSuccessEvent {
    pub name: String,
    pub message: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorActionFailedEvent {
    pub name: String,
    pub message: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratorCompletedEvent {
    pub name: String,
}
