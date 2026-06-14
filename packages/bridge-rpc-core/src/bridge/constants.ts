/**
 * Buffer sizes for the bounded channels used by the bridge. These mirror the
 * Rust `bridge_rpc_core` constants so both implementations apply backpressure
 * at the same thresholds.
 */

/**
 * Capacity of the outbound frame worker channel that feeds the transport
 * writer. Mirrors Rust's `BYTES_WORKER_BUFFER_SIZE`.
 */
export const FRAME_WORKER_BUFFER_SIZE = 256;

/**
 * Capacity of the per-session request/response body frame channels. Mirrors
 * Rust's `RESPONSE_BUFFER_SIZE`.
 */
export const RESPONSE_BUFFER_SIZE = 256;
