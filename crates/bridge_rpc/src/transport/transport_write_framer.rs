use std::marker::PhantomData;

use bytes::{BufMut as _, Bytes, BytesMut};

use super::constants::LENGTH_PREFIX_LENGTH;

#[derive(Debug, Clone, Copy, Default)]
pub struct TransportWriteFramer {
    _private: PhantomData<()>,
}

impl TransportWriteFramer {
    pub fn new() -> Self {
        Self {
            _private: PhantomData,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Framed {
    pub length: Bytes,
    pub data: Bytes,
}

impl TransportWriteFramer {
    pub fn frame(&self, bytes: Bytes) -> Framed {
        let length = bytes.len();
        let mut length_bytes = BytesMut::with_capacity(LENGTH_PREFIX_LENGTH);

        length_bytes.put_u32_le(length as u32);

        Framed {
            length: length_bytes.freeze(),
            data: bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{LittleEndian, ReadBytesExt};
    use bytes::{Buf, Bytes};

    #[test]
    fn test_frame() {
        let transport_write_framer = TransportWriteFramer::new();
        let bytes = Bytes::from("hello world");

        let framed = transport_write_framer.frame(bytes.clone());
        let length_bytes_length = framed.length.len();
        let read_data_length = framed
            .length
            .reader()
            .read_u32::<LittleEndian>()
            .expect("should be able to read from the buffer");

        assert_eq!(length_bytes_length, LENGTH_PREFIX_LENGTH);
        assert_eq!(read_data_length, framed.data.len() as u32);
        assert_eq!(framed.data, bytes);
    }
}
