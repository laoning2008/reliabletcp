#![feature(split_array)]

extern crate core;

mod packet;
pub use packet::Header;
pub use packet::Packet;
pub use packet::SendOption;
pub use packet::Request;

mod error;
pub use error::Error;

mod codec;
pub use codec::Codec;

mod handler;
pub use handler::PacketHandler;

pub mod util;