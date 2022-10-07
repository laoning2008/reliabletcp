use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, Notify};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::runtime;
use reliabletcp_common::{util, Header, Packet, Codec, PacketHandler};
use tokio_util::codec::{ Decoder};
use tokio_stream::StreamExt;
use futures::SinkExt;

pub struct Connection {
    runtime: &'static runtime::Runtime,
    stop_notifier: Arc<Notify>,
    started: AtomicBool,
    device_id: Mutex<String>,
    handlers: Arc<HashMap<u32, PacketHandler>>,
    last_received_packet_time: AtomicU64,
    packet_sender: Mutex<Option<mpsc::Sender<Packet>>>,
    sequence_generator: AtomicU64,
}

impl Connection {
    pub fn new(runtime: &'static runtime::Runtime, socket: TcpStream, handlers: Arc<HashMap<u32, PacketHandler>>,) -> Arc<Self> {
        println!("new connection");

        let the = Arc::new(Connection {
            runtime,
            stop_notifier: Arc::new(Notify::new()),
            started: AtomicBool::new(false),
            device_id: Mutex::new(String::default()),
            handlers,
            last_received_packet_time: AtomicU64::new(util::get_current_timestamp()),
            packet_sender: Mutex::new(None),
            sequence_generator: AtomicU64::new(util::get_current_timestamp()),
        });

        let the_clone = the.clone();
        tokio::spawn(async move {
            let the = the_clone.clone();
            the.started.store(true, Ordering::SeqCst);
            the_clone.run_loop(socket).await;
            the.started.store(false, Ordering::SeqCst);
        });

        the
    }

    pub fn last_received_packet_time(&self) -> u64 {
        self.last_received_packet_time.load(Ordering::SeqCst)
    }

    pub async fn device_id(&self) -> String {
        self.device_id.lock().await.clone()
    }

    pub async fn response(self: Arc<Self>, req_header: Header, body: Vec<u8>) -> bool {
        let sender = self.packet_sender.lock().await;
        if sender.is_none() {
            return false;
        }
        sender.clone().unwrap().send(Packet {header: req_header, body}).await.is_ok()
    }

    pub async fn push(self: Arc<Self>, cmd: u32, encrypted: bool, body: Vec<u8>) -> bool {
        let sender = self.packet_sender.lock().await;
        if sender.is_none() {
            return false;
        }

        let header = Header::new(cmd, self.sequence_generator.fetch_add(1, Ordering::SeqCst), self.device_id().await, true, encrypted);
        sender.clone().unwrap().send(Packet {header, body}).await.is_ok()
    }

    async fn run_loop(self: Arc<Self>, socket: TcpStream) {
        let (tx, mut rx) = mpsc::channel(32);
        let mut sender = self.packet_sender.lock().await;
        *sender = Some(tx);
        drop(sender);

        let mut framed = Codec::new().framed(socket);
        loop {
            tokio::select! {
                packet = framed.next() => {
                    if let Some(packet) = packet {
                        match packet {
                            Ok(packet) => {//got packet
                                println!("receive a packet, cmd = {}, seq = {}", packet.header.cmd, packet.header.seq);
                                let self_clone = self.clone();
                                self_clone.process_packet(packet).await;
                            },
                            Err(err) => {//error
                                println!("receive error = {err}");
                                break;
                            }
                        }
                    } else {//stream finished
                        println!("stream finished");
                        break;
                    }
                }
                packet = rx.recv() => {
                    if let Some(packet) = packet {
                        println!("receive a packet need to be sent");
                        let send_result = framed.send(packet).await;
                        if send_result.is_err() {
                            println!("send failed");
                        }
                    } else {
                        println!("receive send request failed");
                        break;
                    }
                }
                _ = self.stop_notifier.notified() => {
                    println!("stop notification");
                    break;
                }
            }
        }
    }

    async fn process_packet(self: Arc<Self>, packet: Packet) {
        if packet.header.device_id.is_empty() {
            return;
        }

        let mut device_id = self.device_id.lock().await;
        if device_id.is_empty() {
            *device_id = packet.header.device_id.clone();
            drop(device_id);
        } else if packet.header.device_id != device_id.clone() {
            return;
        }

        self.last_received_packet_time.store(util::get_current_timestamp(), Ordering::SeqCst);

        let handler = self.handlers.get(&packet.header.cmd);
        if handler.is_none() {
            return;
        }

        let handler = handler.unwrap();
        handler(packet).await;
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        println!("connection drop");
        if !self.started.load(Ordering::SeqCst) {
            println!("connection has already stopped");
            return;
        }

        self.runtime.block_on(async {
            self.stop_notifier.notify_waiters();

            while self.started.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
    }
}
