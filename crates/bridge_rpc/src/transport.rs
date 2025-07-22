use std::fmt::Display;

#[cfg_attr(test, mockall::automock(type Error = String;))]
#[async_trait::async_trait]
pub trait Transport {
    type Error: Display;

    async fn send(&self, data: Vec<u8>) -> Result<(), Self::Error>;
    async fn receive(&self) -> Result<Vec<u8>, Self::Error>;
}
