use async_trait::async_trait;
use bytes::Bytes;
use std::fmt::Display;

#[async_trait]
#[cfg_attr(test, mockall::automock(type Error = String;))]
pub trait Transport: Send + Sync + 'static {
    type Error: Display + Send + Sync + 'static;

    async fn send(&self, data: Bytes) -> Result<(), Self::Error>;
    async fn receive(&self) -> Result<Bytes, Self::Error>;
}
