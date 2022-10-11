use std::mem::size_of;
use std::str::from_utf8;
use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use crate::{Error, Header, Packet, util};

const HEADER_LEN: usize = 52;//size_of::<HeaderCodec>();
const DEVICE_ID_LENGTH: usize = 32;
const FLAG: u8 = 0x1A;
const POLYNOMIAL: u8 = 0xD5;
const MAX_BODY_LEN: u32 = 8*1024;

#[repr(align(1))]
struct HeaderCodec {
    pub flag: u8,
    pub cmd: u32,
    pub seq: u64,
    pub device_id: [u8; DEVICE_ID_LENGTH],
    pub push: u8,
    pub encrypted: u8,
    pub body_len: u32,
    pub crc: u8,
}


#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Codec {

}

impl Codec {
    pub fn new() -> Codec {
        Codec {
        }
    }
}


impl Decoder for Codec {
    type Item = Packet;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Packet>, Error> {
        loop {
            let packet_start_pos = buf[0..buf.len()].iter().position(|b| *b == FLAG);
            if let Some(packet_start_pos) = packet_start_pos {
                buf.advance(packet_start_pos);
            } else {
                buf.advance(buf.len());
                return Ok(None);
            }

            if buf.len() < HEADER_LEN {
                return Ok(None);
            }

            let (header_buf, _) = buf.split_array_ref::<HEADER_LEN>();
            if !crc8_rs::has_valid_crc8(*header_buf, POLYNOMIAL) {
                buf.advance(size_of::<u8>());//skip flag
                continue;
            }

            let mut buf_header = BytesMut::from(header_buf.as_ref());

            let _ = buf_header.get_u8();//flag
            let cmd = buf_header.get_u32();
            let seq = buf_header.get_u64();
            let device_id = buf_header.split_to(DEVICE_ID_LENGTH);
            let push = buf_header.get_u8();
            let encrypted = buf_header.get_u8();
            let body_len = buf_header.get_u32();
            let _ = buf_header.get_u8();//crc
            if body_len > MAX_BODY_LEN {
                buf.advance(HEADER_LEN);
                continue;
            }

            if buf.len() < HEADER_LEN + body_len as usize {
                return Ok(None);
            }

            buf.advance(HEADER_LEN);
            let body = buf.split_to(body_len as usize);

            let body = body.to_vec();
            let header = Header::new(cmd, seq,  from_utf8(device_id.as_ref()).unwrap_or("").to_string(), util::u8_to_bool(push), util::u8_to_bool(encrypted));
            let packet = Packet {header, body};
            return Ok(Some(packet));
        }
    }
}

impl Encoder<Packet> for Codec
{
    type Error = Error;

    fn encode(&mut self, packet: Packet, buf: &mut BytesMut) -> Result<(), Error> {
        if packet.header.device_id.len() != DEVICE_ID_LENGTH {
            return Err(Error::DECODE);
        }

        let mut buf_header = BytesMut::new();
        buf_header.reserve(HEADER_LEN);
        buf_header.put_u8(FLAG);
        buf_header.put_u32(packet.header.cmd);
        buf_header.put_u64(packet.header.seq);
        buf_header.put_slice(packet.header.device_id.as_bytes());
        buf_header.put_u8(util::bool_to_u8(packet.header.push));
        buf_header.put_u8(util::bool_to_u8(packet.header.encrypted));
        buf_header.put_u32(packet.body.len() as u32);
        buf_header.put_u8(0);//hold the crc place

        let (mut header, _) = buf_header.split_array_ref::<HEADER_LEN>();
        let header = crc8_rs::insert_crc8(*header, POLYNOMIAL);


        buf.reserve(size_of::<HeaderCodec>() + packet.body.len());
        buf.put_slice(&header);
        buf.put_slice(&packet.body);

        Ok(())
    }
}

impl Default for Codec {
    fn default() -> Self {
        Self::new()
    }
}
