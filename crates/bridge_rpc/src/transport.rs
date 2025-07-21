use std::fmt::Display;

#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    type Error: Display;

    async fn send(&self, data: Vec<u8>) -> Result<(), Self::Error>;
    async fn receive(&self) -> Result<Vec<u8>, Self::Error>;
}
