//! Shared helpers used by the file-system and process services.
//!
//! Wire conventions
//! -----------------
//!
//! Trivial / small payloads are sent in headers, under a single well-known
//! key. Two distinct keys are used depending on the direction of the call:
//!
//! - Requests carry their input under [`PARAMETERS_HEADER`] (`"parameters"`)
//!   - the *parameters* of the call.
//! - Responses carry their output under [`RETURNS_HEADER`] (`"returns"`)
//!   - the *return value* of the call.
//!
//! The value in either case is an `rmpv::Value` map carrying the parameter
//! / return struct: encoding to MessagePack is handled by the underlying
//! protocol so callers should never re-serialize values to JSON or any
//! other intermediate format.
//!
//! Bulk binary or textual content (file contents, etc.) goes in the request
//! or response **body**. Bodies that are larger than [`MAX_CHUNK_SIZE`] are
//! split into fixed-size chunks via successive `write_body_chunk` calls;
//! reading is symmetric and assembles the chunks back into a single
//! buffer.
use bridge_rpc_core::{
    DynMap, Headers, ResponseStatusCode,
    server::{
        request::{Request, RequestReader},
        response::PendingResponse,
    },
    service_error::ServiceError,
};

/// Header key carrying the per-call parameters object on a **request**.
pub const PARAMETERS_HEADER: &str = "parameters";

/// Header key carrying the per-call return value on a **response**.
pub const RETURNS_HEADER: &str = "returns";

/// Maximum size, in bytes, of a single body chunk.
///
/// Bodies larger than this are split into multiple `BodyChunk` frames when
/// being written, and assembled from multiple frames when being read.
pub const MAX_CHUNK_SIZE: usize = 64 * 1024;

// ---------------------------------------------------------------------------
// Parameter (request header) helpers
// ---------------------------------------------------------------------------

/// Decodes the `parameters` request header into `T`.
///
/// Returns an error if the header is missing or cannot be deserialized into
/// the expected shape.
pub fn read_parameters<T>(headers: Option<&Headers>) -> Result<T, ServiceError>
where
    T: serde::de::DeserializeOwned,
{
    let value = headers
        .and_then(|h| h.get_raw(PARAMETERS_HEADER))
        .ok_or_else(|| {
            ServiceError::custom_error(eyre::eyre!(
                "missing `{PARAMETERS_HEADER}` header"
            ))
        })?;

    rmpv::ext::from_value::<T>(value.clone())
        .map_err(ServiceError::custom_error)
}

/// Decodes the `parameters` request header into `T`, returning `Ok(None)` if
/// the header is absent.
pub fn read_optional_parameters<T>(
    headers: Option<&Headers>,
) -> Result<Option<T>, ServiceError>
where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = headers.and_then(|h| h.get_raw(PARAMETERS_HEADER)) else {
        return Ok(None);
    };

    rmpv::ext::from_value::<T>(value.clone())
        .map(Some)
        .map_err(ServiceError::custom_error)
}

/// Encodes `value` into a [`Headers`] map containing only the `parameters`
/// entry, suitable for use as **request** headers.
pub fn encode_parameters<T>(value: &T) -> Result<Headers, ServiceError>
where
    T: serde::Serialize,
{
    let mut headers = DynMap::new();
    headers
        .insert(PARAMETERS_HEADER, value)
        .map_err(ServiceError::custom_error)?;
    Ok(headers)
}

// ---------------------------------------------------------------------------
// Return-value (response header) helpers
// ---------------------------------------------------------------------------

/// Decodes the `returns` response header into `T`.
///
/// Returns an error if the header is missing or cannot be deserialized into
/// the expected shape.
pub fn read_returns<T>(headers: Option<&Headers>) -> Result<T, ServiceError>
where
    T: serde::de::DeserializeOwned,
{
    let value =
        headers
            .and_then(|h| h.get_raw(RETURNS_HEADER))
            .ok_or_else(|| {
                ServiceError::custom_error(eyre::eyre!(
                    "missing `{RETURNS_HEADER}` header"
                ))
            })?;

    rmpv::ext::from_value::<T>(value.clone())
        .map_err(ServiceError::custom_error)
}

/// Decodes the `returns` response header into `T`, returning `Ok(None)` if
/// the header is absent.
pub fn read_optional_returns<T>(
    headers: Option<&Headers>,
) -> Result<Option<T>, ServiceError>
where
    T: serde::de::DeserializeOwned,
{
    let Some(value) = headers.and_then(|h| h.get_raw(RETURNS_HEADER)) else {
        return Ok(None);
    };

    rmpv::ext::from_value::<T>(value.clone())
        .map(Some)
        .map_err(ServiceError::custom_error)
}

/// Encodes `value` into a [`Headers`] map containing only the `returns`
/// entry, suitable for use as **response** headers.
pub fn encode_returns<T>(value: &T) -> Result<Headers, ServiceError>
where
    T: serde::Serialize,
{
    let mut headers = DynMap::new();
    headers
        .insert(RETURNS_HEADER, value)
        .map_err(ServiceError::custom_error)?;
    Ok(headers)
}

// ---------------------------------------------------------------------------
// Body helpers
// ---------------------------------------------------------------------------

/// Reads the entire body of a [`Request`] into a single `Vec<u8>`.
///
/// Frames are accumulated in order; all chunks are concatenated.
pub async fn read_full_body(request: Request) -> Result<Vec<u8>, ServiceError> {
    let mut reader = request.into_reader();
    read_full_body_from_reader(&mut reader).await
}

/// Reads the entire body of a [`RequestReader`] into a single `Vec<u8>`.
pub async fn read_full_body_from_reader(
    reader: &mut RequestReader,
) -> Result<Vec<u8>, ServiceError> {
    let mut acc: Vec<u8> = Vec::new();
    while let Some(chunk) = reader
        .read_body_chunk()
        .await
        .map_err(ServiceError::custom_error)?
    {
        acc.extend_from_slice(&chunk);
    }
    Ok(acc)
}

// ---------------------------------------------------------------------------
// Response helpers
// ---------------------------------------------------------------------------

/// Starts a successful response, writes a single `returns` header derived
/// from `value`, and ends without a body.
pub async fn respond_with_returns<T>(
    response: PendingResponse,
    value: &T,
) -> Result<(), ServiceError>
where
    T: serde::Serialize,
{
    let headers = encode_returns(value)?;

    response
        .start_with_headers(ResponseStatusCode::SUCCESS, headers)
        .await
        .map_err(ServiceError::custom_error)?
        .end()
        .await
        .map_err(ServiceError::custom_error)?;

    Ok(())
}

/// Starts a successful response with no headers and no body.
pub async fn respond_empty(
    response: PendingResponse,
) -> Result<(), ServiceError> {
    response
        .start(ResponseStatusCode::SUCCESS)
        .await
        .map_err(ServiceError::custom_error)?
        .end()
        .await
        .map_err(ServiceError::custom_error)?;
    Ok(())
}

/// Starts a successful response with no headers and the supplied body,
/// chunking the body when it exceeds [`MAX_CHUNK_SIZE`].
pub async fn respond_with_body(
    response: PendingResponse,
    body: &[u8],
) -> Result<(), ServiceError> {
    let mut active = response
        .start(ResponseStatusCode::SUCCESS)
        .await
        .map_err(ServiceError::custom_error)?;

    for chunk in body.chunks(MAX_CHUNK_SIZE) {
        active
            .write_body_chunk(chunk.to_vec())
            .await
            .map_err(ServiceError::custom_error)?;
    }

    active.end().await.map_err(ServiceError::custom_error)?;
    Ok(())
}
