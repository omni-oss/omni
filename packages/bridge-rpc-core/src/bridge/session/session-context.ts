/**
 * Marker interface for session contexts.
 *
 * A context may optionally implement {@link close} to release its underlying
 * channels when the owning session is closed, so that consumers blocked on
 * those channels observe EOF instead of hanging. This mirrors the Rust
 * implementation, where dropping the senders on close terminates the
 * corresponding receivers.
 */
export interface ClosableSessionContext {
    close?(): void | Promise<void>;
}
