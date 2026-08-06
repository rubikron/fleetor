//! `fleetor-core` — the frozen contracts every other crate speaks (BUILDING §4).
//!
//! Pure domain types, no I/O beyond serde. Defined and versioned *before* any
//! logic so the wire surface (event stream, message framing, DB shape) can't
//! drift as internals churn.
//!
//! **This is what is left after Phase 5.** The headless surface —
//! envelope/mail/ticket/report/review/gate/ownership — is gone with the
//! supervisor that used it. A TUI fleet has one kind of thing to say
//! ([`message`]), one kind of thing to *do* to a terminal that is not saying
//! anything to it ([`command`]), one kind of thing to be ([`pane`]), one thing to
//! tell each pane at spawn ([`brief`]), one thing to write down together
//! ([`task`]), and a log to write it all to ([`event`], [`store`]).
//!
//! [`task`] is the one that has to keep proving itself: a board of work is the
//! shape the deleted ticket system had, and the only reason this one is not that
//! is that nothing reads it back to decide anything. Its module doc carries the
//! tripwire list.

pub mod brief;
pub mod command;
pub mod event;
pub mod ids;
pub mod message;
pub mod pane;
pub mod store;
pub mod task;
pub mod time;
pub mod wire;

pub use brief::{orch_brief, worker_brief, VERBS};
pub use command::{Command, ALLOWED_COMMANDS};
pub use event::{FleetEvent, NoticeLevel};
pub use message::{frame_broadcast_for_pane, frame_for_pane, Message};
pub use pane::{ContextGauge, PaneEntry, PaneId, PaneState, ParsePaneIdError, WORKER_SLOTS};
pub use store::Store;
pub use task::{board, TaskBlock, TaskEntry, TaskStatus, TaskUpdate, TASK_STATUSES};
pub use wire::{Hello, Op, OpResult, Request, Response, TaskAction, WIRE_VERSION};
