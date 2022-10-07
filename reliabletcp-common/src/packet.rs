use tokio::sync::oneshot;
use crate::{util, Error};

pub struct Header {
    pub cmd: u32,
    pub seq: u64,
    pub device_id: String,
    pub push: bool,
    pub encrypted: bool,
}

impl Header {
    pub fn new(cmd: u32, seq: u64, device_id: String, push: bool, encrypted: bool) -> Self {
        Header {
            cmd,
            seq,
            device_id,
            push,
            encrypted,
        }
    }
}

impl Clone for Header {
    fn clone(&self) -> Self {
        Header {
            cmd: self.cmd,
            seq: self.seq,
            device_id: self.device_id.clone(),
            push: self.push,
            encrypted: self.encrypted,
        }
    }
}

pub struct Packet {
    pub header: Header,
    pub body: Vec<u8>,
}

impl Clone for Packet {
    fn clone(&self) -> Self {
        Packet {
            header: self.header.clone(),
            body: self.body.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SendOption {
    pub resend_times: u32,
    pub resend_interval: u64,
}

impl Default for SendOption {
    fn default() -> Self {
        SendOption {
            resend_times: 3,
            resend_interval: 3 * 1000,
        }
    }
}

impl SendOption {
    pub fn new(resend_times: u32, resend_interval: u64) -> Self {
        SendOption {
            resend_times,
            resend_interval,
        }
    }

    pub fn no_resend() -> Self {
        SendOption {
            resend_times: 0,
            resend_interval: 3 * 1000,
        }
    }

    pub fn should_resend(&self) -> bool {
        self.resend_times > 0
    }
}

pub struct Request {
    pub packet: Packet,
    pub option: SendOption,
    pub last_send_time: u64,
    pub resent_times: u32,
    pub notifier: oneshot::Sender<Result<Packet, Error>>,
}

impl Request {
    pub fn new(packet: Packet, option: SendOption, notifier: oneshot::Sender<Result<Packet, Error>>) -> Request {
        Request {packet, option, last_send_time: util::get_current_timestamp(), resent_times: 0, notifier}
    }
}