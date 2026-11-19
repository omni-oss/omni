//! The stored form of a remote cache entry (RFC 0030, section 4.3).
//!
//! One object holds the artifact and its provenance, so they commit together:
//!
//! ```text
//! [artifact: tar.gz bytes][trailer: msgpack][u32 BE trailer_len][magic "OMNIENT1"]
//! ```
//!
//! [`Trailer`] describes and checks that layout, [`seal`] writes it while an
//! upload streams, and [`read_entry`] and [`checked_body`] read it back. Only the server writes a trailer, and nothing here reads
//! request data except the artifact bytes and the client's declared SHA-256.

mod read;
mod seal;
mod trailer;

pub use read::*;
pub use seal::*;
pub use trailer::*;
