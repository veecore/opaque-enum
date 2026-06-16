//! Opaque enum support.
//!
//! This crate exposes the [`opaque_enum`] attribute macro plus the projection
//! trait used by generated forwarding impls. The macro lets a public type keep
//! an enum-like authoring experience while exposing an opaque wrapper instead of
//! public enum variants.
//!
//! # Example
//!
//! ```rust
//! use opaque_enum::opaque_enum;
//! use std::fmt::{self, Display, Formatter};
//!
//! #[opaque_enum]
//! #[derive(Debug)]
//! pub enum DatabaseError {
//!     ConnectionFailed(String),
//!     QueryFailed { query: String, reason: String },
//!     PermissionDenied,
//! }
//!
//! #[opaque_enum]
//! impl Display for DatabaseError {
//!     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
//!         match self {
//!             Self::ConnectionFailed(err) => write!(f, "connection failed: {err}"),
//!             Self::QueryFailed { query, reason } => {
//!                 write!(f, "query `{query}` failed: {reason}")
//!             }
//!             Self::PermissionDenied => write!(f, "permission denied"),
//!         }
//!     }
//! }
//!
//! let err = DatabaseError::ConnectionFailed("timeout".to_owned());
//! assert_eq!(err.to_string(), "connection failed: timeout");
//! ```

#![deny(missing_docs)]

pub use opaque_enum_macros::opaque_enum;

/// Projects an opaque wrapper receiver into the corresponding inner receiver.
///
/// Generated forwarding impls use this trait instead of hard-coding whether a
/// receiver should call `as_inner`, `as_inner_mut`, or `into_inner`.
pub trait OpaqueProject<Target> {
    /// The projected receiver type passed to the inner implementation.
    type Output;

    /// Projects `self` into the receiver expected by the inner implementation.
    fn project(self) -> Self::Output;
}
