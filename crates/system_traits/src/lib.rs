#![feature(decl_macro)]
#![allow(unused_imports)]

#[cfg(feature = "async")]
mod asynch;

#[cfg(feature = "sync")]
mod synch;

pub mod impls;
mod shared;

#[cfg(feature = "async")]
pub use asynch::*;

#[cfg(feature = "sync")]
pub use synch::*;

pub use shared::*;
