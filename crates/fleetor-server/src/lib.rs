//! `fleetor-server` — the routing core of a FLEETOR fleet (BUILDING §3).
//!
//! Two modules after Phase 5. [`hub`] is the server side of the fleet socket:
//! it decides where a message goes, asks the app to type it, and writes down
//! what happened. [`bus`] is the live push side of the event log —
//! [`BroadcastStore`] wraps the [`Store`] seam so every appended event streams to
//! subscribers, which is what the UI's feed consumes.
//!
//! The supervisor, the runner, the quality loop and the shell gate are gone with
//! the headless fleet they drove. There is nothing to supervise: a pane is a
//! terminal the operator can see, and its agent answers to them.
//!
//! [`Store`]: fleetor_core::Store

pub mod bus;
pub mod hub;

pub use bus::{BroadcastStore, EventBus, EventFollower, BUS_CAPACITY};
pub use hub::{AppCommand, DeliveryResult, Hub, Interview};
