use std::pin::Pin;
use crate::Packet;
use std::future::Future;

pub type PacketHandler = Box<dyn Fn(Packet) -> Pin<Box<(dyn Future<Output = bool> + Send + 'static)>> + Send + Sync + 'static>;

