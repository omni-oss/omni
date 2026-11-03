use derive_new::new;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[allow(unused)]
#[derive(
    Deserialize,
    Serialize,
    new,
    ToSchema,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
pub struct Data<T> {
    data: T,
}

impl<T: Serialize> From<T> for Data<T> {
    #[inline(always)]
    fn from(data: T) -> Self {
        Self::new(data)
    }
}

#[allow(unused)]
#[derive(Deserialize, Serialize, new, ToSchema)]
pub struct PagedData<T> {
    data: Vec<T>,
    page_size: u32,
    total_size: u32,
    has_next: bool,
    has_previous: bool,
}
