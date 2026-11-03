#![feature(decl_macro)]
#![allow(clippy::redundant_field_names)]

pub mod config;
pub mod data;
pub mod data_impl;
mod extractors;
pub mod providers;
pub mod request;
pub mod response;
pub mod routes;
mod scalar;
mod security;
pub mod services;
pub mod state;
mod utils;
