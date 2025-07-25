use byteorder::{ByteOrder, LittleEndian};
use bytes::{Buf, BufMut, Bytes, BytesMut};

/// The length of the prefix indicating the frame's total length.
/// Corresponds to `LENGTH_PREFIX_LENGTH` in your TypeScript.
const LENGTH_PREFIX_LENGTH: usize = 4; // Assuming a Uint32, which is 4 bytes

pub struct TransportReadFramer {
    /// Buffer for collecting the current frame's bytes.
    current_frame_bytes: BytesMut,
    /// The expected length of the current frame, once the prefix is read.
    current_expected_frame_length: Option<usize>,

    /// The number of bytes of the length prefix already buffered.
    prefix_buffered_length: usize,
    /// The number of bytes of the frame body already buffered.
    frame_buffered_length: usize,
}

impl Default for TransportReadFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportReadFramer {
    pub fn new() -> Self {
        TransportReadFramer {
            current_frame_bytes: BytesMut::new(),
            current_expected_frame_length: None,
            prefix_buffered_length: 0,
            frame_buffered_length: 0,
        }
    }

    /// Processes incoming bytes and returns a vector of complete frames.
    ///
    /// If no complete frames are found, it returns `None`.
    pub fn frame(&mut self, mut bytes: Bytes) -> Option<Vec<Bytes>> {
        let mut frames: Vec<Bytes> = Vec::new();

        while bytes.has_remaining() {
            // Collect length prefix first
            if self.current_expected_frame_length.is_none() {
                let needed = LENGTH_PREFIX_LENGTH - self.prefix_buffered_length;
                let chunk_len = (bytes.remaining()).min(needed);

                if chunk_len > 0 {
                    // Append chunk to current_frame_bytes which temporarily holds prefix bytes
                    let prefix_chunk = bytes.split_to(chunk_len);
                    self.current_frame_bytes.put(prefix_chunk);
                    self.prefix_buffered_length += chunk_len;
                }

                if self.prefix_buffered_length == LENGTH_PREFIX_LENGTH {
                    // Read the length from the collected prefix bytes
                    let full_prefix =
                        self.current_frame_bytes.split_to(LENGTH_PREFIX_LENGTH);
                    let length = LittleEndian::read_u32(&full_prefix) as usize;
                    self.current_expected_frame_length = Some(length);
                    self.prefix_buffered_length = 0;
                } else {
                    // Not enough bytes for the prefix yet, return and wait for more.
                    // The collected bytes remain in `current_frame_bytes`
                    return None; // No complete frame can be formed yet
                }
            }

            // Now collect the frame body
            if let Some(expected_length) = self.current_expected_frame_length {
                let needed = expected_length - self.frame_buffered_length;
                let chunk_len = (bytes.remaining()).min(needed);

                if chunk_len > 0 {
                    // Append chunk to the current frame buffer
                    let chunk = bytes.split_to(chunk_len);
                    self.current_frame_bytes.put(chunk); // Efficiently appends Bytes to BytesMut
                    self.frame_buffered_length += chunk_len;
                }

                if self.frame_buffered_length == expected_length {
                    // A complete frame has been received
                    frames.push(
                        self.current_frame_bytes
                            .split_to(expected_length)
                            .freeze(),
                    );
                    self.current_expected_frame_length = None;
                    self.frame_buffered_length = 0;
                } else {
                    // Not enough bytes for the frame body yet, return and wait for more.
                    return None; // No complete frame can be formed yet
                }
            }
        }

        if frames.is_empty() {
            None
        } else {
            Some(frames)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a length prefix Bytes object
    fn create_length_prefix(length: u32) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u32_le(length);
        buf.freeze()
    }

    /// Helper function to combine prefix and data into a single Bytes object
    fn combine_prefix_and_data(data: &[u8]) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u32_le(data.len() as u32);
        buf.put_slice(data);
        buf.freeze()
    }

    #[test]
    fn test_should_be_able_to_frame_data_in_normal_order() {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::from_static(&[1, 2, 3, 4]);

        let length_prefix = create_length_prefix(data.len() as u32);

        // First, provide the length prefix
        assert!(framer.frame(length_prefix).is_none()); // Expect no complete frame yet

        // Then, provide the actual data
        let framed = framer.frame(data.clone()).unwrap();
        assert_eq!(framed, vec![data]);
    }

    #[test]
    fn test_should_return_none_if_no_frame_is_complete() {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::from_static(&[1]); // Only 1 byte, but expected length is 4

        let combined = combine_prefix_and_data(&data);

        // Provide insufficient data
        let framed = framer.frame(combined.slice(0..combined.len() - 1)); // Send almost all, but not quite
        assert!(framed.is_none());

        // Even after sending just the length prefix and a single byte, it's not complete
        let mut framer_partial_data = TransportReadFramer::new();
        let len_prefix = create_length_prefix(4); // Expect 4 bytes
        assert!(framer_partial_data.frame(len_prefix).is_none());
        assert!(
            framer_partial_data
                .frame(Bytes::from_static(&[1]))
                .is_none()
        ); // Only 1 byte of 4
    }

    #[test]
    fn test_should_be_able_to_frame_data_in_a_single_received_byte_array() {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::from_static(&[1, 2, 3, 4]);

        let combined = combine_prefix_and_data(&data);

        let framed = framer.frame(combined).unwrap();
        assert_eq!(framed, vec![data]);
    }

    #[test]
    fn test_should_be_able_to_frame_data_with_partial_length_prefix_first() {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::from_static(&[1]); // Data length is 1
        let expected_length = data.len();

        let combined_full_message = combine_prefix_and_data(&data);

        // Split the combined message as per JS test: prefix (3 bytes), prefix (1 byte), data (1 byte)
        let bytes_parts = vec![
            combined_full_message.slice(0..3), // Partial prefix
            combined_full_message.slice(3..),  // Other prefix + data
        ];

        let mut framed_results: Vec<Bytes> = Vec::new();
        for byte_part in bytes_parts {
            if let Some(frames) = framer.frame(byte_part) {
                framed_results.extend(frames);
            }
        }

        assert_eq!(framed_results[0].len(), expected_length);
        assert_eq!(framed_results, vec![data]);
    }

    #[test]
    fn test_should_be_able_to_frame_data_in_a_interleaved_byte_arrays() {
        let mut framer = TransportReadFramer::new();

        let data = Bytes::from_static(&[1, 2, 3, 4]); // Data length is 4
        let combined_full_message = combine_prefix_and_data(&data);

        // Split to 2, 4, 2 bytes (this implicitly means partial prefix, then rest of prefix + partial data, then rest of data)
        let bytes_parts = vec![
            combined_full_message.slice(0..2),
            combined_full_message.slice(2..6),
            combined_full_message.slice(6..),
        ];

        let mut framed_results: Vec<Bytes> = Vec::new();
        for byte_part in bytes_parts {
            if let Some(frames) = framer.frame(byte_part) {
                framed_results.extend(frames);
            }
        }

        assert_eq!(framed_results, vec![data]);
    }

    #[test]
    fn test_should_be_able_to_frame_multiple_data_in_a_single_byte_array() {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::from_static(&[1, 2, 3, 4]);

        let mut combined_mut = BytesMut::new();
        combined_mut.put_u32_le(data.len() as u32);
        combined_mut.put_slice(&data);
        combined_mut.put_u32_le(data.len() as u32);
        combined_mut.put_slice(&data);
        let combined = combined_mut.freeze();

        let framed = framer.frame(combined).unwrap();
        assert_eq!(framed, vec![data.clone(), data]);
    }

    #[test]
    fn test_should_be_able_to_frame_multiple_data_in_an_interleaved_byte_array()
    {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::from_static(&[1, 2, 3, 4]); // Data length is 4

        let mut combined_mut = BytesMut::new();
        combined_mut.put_u32_le(data.len() as u32);
        combined_mut.put_slice(&data);
        combined_mut.put_u32_le(data.len() as u32);
        combined_mut.put_slice(&data);
        let combined_full_message = combined_mut.freeze(); // Total 16 bytes (4+4 for first, 4+4 for second)

        // Split to 2, 4, 2, 2, 4, 2 bytes
        let bytes_parts = vec![
            combined_full_message.slice(0..2), // Partial prefix 1
            combined_full_message.slice(2..6), // Rest prefix 1 + partial data 1
            combined_full_message.slice(6..8), // Rest data 1 + partial prefix 2
            combined_full_message.slice(8..10), // Rest prefix 2
            combined_full_message.slice(10..14), // Partial data 2
            combined_full_message.slice(14..), // Rest data 2
        ];

        let mut framed_results: Vec<Bytes> = Vec::new();
        for byte_part in bytes_parts {
            if let Some(frames) = framer.frame(byte_part) {
                framed_results.extend(frames);
            }
        }

        assert_eq!(framed_results, vec![data.clone(), data]);
    }

    #[test]
    fn test_should_be_able_to_frame_data_with_zero_length_prefix() {
        let mut framer = TransportReadFramer::new();
        let data = Bytes::new(); // Empty Bytes object

        let combined = combine_prefix_and_data(&data);

        let framed = framer.frame(combined).unwrap();
        assert_eq!(framed, vec![data]);
    }
}
