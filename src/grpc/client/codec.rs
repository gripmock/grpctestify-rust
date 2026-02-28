use prost::Message;
use prost_reflect::{DynamicMessage, MessageDescriptor};
use tonic::Status;
use tonic::codec::{Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

pub struct DynamicCodec {
    _input_desc: MessageDescriptor,
    output_desc: MessageDescriptor,
}

impl DynamicCodec {
    pub fn new(input_desc: MessageDescriptor, output_desc: MessageDescriptor) -> Self {
        Self {
            _input_desc: input_desc,
            output_desc,
        }
    }
}

impl Codec for DynamicCodec {
    type Encode = DynamicMessage;
    type Decode = DynamicMessage;
    type Encoder = DynamicEncoder;
    type Decoder = DynamicDecoder;

    fn encoder(&mut self) -> Self::Encoder {
        DynamicEncoder::new(self._input_desc.clone())
    }

    fn decoder(&mut self) -> Self::Decoder {
        DynamicDecoder {
            desc: self.output_desc.clone(),
        }
    }
}

pub struct DynamicEncoder {
    _desc: MessageDescriptor,
}

impl DynamicEncoder {
    pub fn new(desc: MessageDescriptor) -> Self {
        Self { _desc: desc }
    }
}

impl Encoder for DynamicEncoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, dst: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(dst)
            .map_err(|e| Status::internal(format!("Encoding error: {}", e)))
    }
}

pub struct DynamicDecoder {
    desc: MessageDescriptor,
}

impl Decoder for DynamicDecoder {
    type Item = DynamicMessage;
    type Error = Status;

    fn decode(&mut self, src: &mut DecodeBuf) -> Result<Option<Self::Item>, Self::Error> {
        let msg = DynamicMessage::decode(self.desc.clone(), src)
            .map_err(|e| Status::internal(format!("Decoding error: {}", e)))?;
        Ok(Some(msg))
    }
}
