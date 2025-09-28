use derive_new::new;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, Serialize, new, ToSchema)]
pub struct Data<T> {
    data: T,
}

impl<T: Serialize> From<T> for Data<T> {
    #[inline(always)]
    fn from(data: T) -> Self {
        Self::new(data)
    }
}
