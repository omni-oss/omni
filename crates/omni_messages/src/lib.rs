// AFIT (async fn in trait) is used throughout this crate. Suppressed because:
// 1. All subscribers are used as generic parameters (S: XxxSubscriber), never dyn.
// 2. Send bounds are enforced by the `Send + Sync` supertrait on each trait.
// 3. dyn compatibility is explicitly deferred until a concrete use case arises.
#![allow(async_fn_in_trait)]

//! Structured event system for the `omni` workspace engine.
//!
//! This crate provides:
//!
//! - **Subscriber traits** ([`ExecutionEventSubscriber`],
//!   [`GeneratorEventSubscriber`], [`OmniEventSubscriber`],
//!   [`DiagnosticSubscriber`]) — implemented by callers to receive structured
//!   events from the engine.
//! - **Event payload types** — serializable structs for every lifecycle event,
//!   organized by subsystem under [`execution`] and [`generator`].
//! - **Built-in subscriber implementations**: [`NoopSubscriber`],
//!   [`ChannelSubscriber`], and (when the `tracing` feature is enabled)
//!   [`TracingSubscriber`]).

pub mod diagnostic;
pub mod execution;
pub mod generator;
pub mod publish;

mod channel;
mod noop;
mod omni;
mod tracing_impl;

// ── Flat re-exports (keep all existing import paths working) ─────────────────

pub use channel::{ChannelSubscriber, OmniEventKind};
pub use diagnostic::{DiagnosticEvent, DiagnosticLevel, DiagnosticSubscriber};
pub use execution::{
    CacheHitEvent, ExecutionCompleteEvent, ExecutionEventSubscriber,
    ExecutionPlanReadyEvent, TaskCompletedEvent, TaskFailedEvent,
    TaskOutputStream, TaskOutputStreamEvent, TaskRetryingEvent, TaskSkipReason,
    TaskSkippedEvent, TaskStartedEvent,
};
pub use generator::{
    GeneratorCompletedEvent, GeneratorEventSubscriber,
    GeneratorFileCreatedEvent, GeneratorFileSkippedEvent, GeneratorStartEvent,
};
pub use noop::NoopSubscriber;
pub use omni::OmniEventSubscriber;

#[cfg(feature = "tracing")]
pub use tracing_impl::TracingSubscriber;
