use std::fmt::Display;

use bytes::Bytes;

#[cfg_attr(test, mockall::automock(type Error = String;))]
#[async_trait::async_trait]
pub trait Transport {
    type Error: Display;

    async fn send(&self, data: Bytes) -> Result<(), Self::Error>;
    async fn receive(&self) -> Result<Bytes, Self::Error>;
}
