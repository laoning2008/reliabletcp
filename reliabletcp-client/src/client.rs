use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::{TcpStream};
use tokio::runtime;
use tokio::sync::{mpsc, Mutex, Notify, oneshot};
use tokio::time;
use reliabletcp_common::{util, Codec, Header, Packet, PacketHandler, Error, SendOption, Request};
use tokio_util::codec::*;
use futures::SinkExt;
use futures::StreamExt;

const RESEND_INTERVAL : u64 = 1 * 1000;
const HEARTBEAT_INTERVAL: u64 = 20 * 1000;


pub struct TcpClient {
    runtime: &'static runtime::Runtime,
    ip: String,
    port: u16,

    device_id: String,
    push_handlers: Arc<HashMap<u32, PacketHandler>>,

    stop_notifier: Arc<Notify>,
    started: AtomicBool,

    packet_sender: mpsc::Sender<Packet>,
    packet_receiver: Mutex<mpsc::Receiver<Packet>>,
    waiting_request: Mutex<HashMap<u128, Request>>,
    sequence_generator: AtomicU64,
}

impl TcpClient {
    pub fn new(runtime: &'static runtime::Runtime, ip: &str, port: u16, device_id: String, push_handlers: Arc<HashMap<u32, PacketHandler>>) -> Arc<Self> {
        let (packet_sender, mut packet_receiver) = mpsc::channel(32);

        Arc::new(TcpClient {
            runtime,
            ip: ip.to_string(),
            port,
            device_id,
            push_handlers,
            stop_notifier: Arc::new(Notify::new()),
            started: AtomicBool::new(false),
            packet_sender,
            packet_receiver: Mutex::new(packet_receiver),
            waiting_request: Mutex::new(HashMap::new()),
            sequence_generator: AtomicU64::new(util::get_current_timestamp()),
        })
    }

    pub async fn start(self: Arc<Self>) {
        if self.started.load(Ordering::SeqCst) {
            return;
        }

        let self_clone = self.clone();
        tokio::spawn(async move {
            self.started.store(true, Ordering::SeqCst);
            println!("client loop started");
            self_clone.run_loop().await;
            println!("client loop stopped");
            self.started.store(false, Ordering::SeqCst);
        });
    }

    pub async fn stop(self: Arc<Self>) {
        if !self.started.load(Ordering::SeqCst) {
            return;
        }

        self.stop_notifier.notify_waiters();
        while self.started.load(Ordering::SeqCst) {
            time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub fn started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }

    pub async fn request(self: Arc<Self>, cmd: u32, encrypted: bool, body: Vec<u8>, option: SendOption) -> Result<Packet, Error> {
        let seq = self.sequence_generator.fetch_add(1, Ordering::SeqCst);
        println!("request, cmd = {cmd}, seq = {seq}");

        let header = Header {
            cmd,
            seq,
            device_id: self.device_id.clone(),
            push: false,
            encrypted,
        };
        let packet = Packet {header, body};
        let send_packet = packet.clone();

        if self.packet_sender.send(send_packet).await.is_err() {
            return Err(Error::Channel);
        }

        let (tx, rx) = oneshot::channel();
        {
            let request_id = self.get_request_id(cmd, seq);
            let mut waiting_request = self.waiting_request.lock().await;
            waiting_request.insert(request_id, Request::new(packet, option, tx));
        }

        let result = rx.await;
        if result.is_err() {
            return Err(Error::Channel);
        }
        result.unwrap()
    }

    async fn run_loop(self: Arc<Self>) {
        let mut exit = false;

        while !exit {
            let socket = TcpStream::connect(format!("{}:{}", self.ip, self.port)).await;
            if socket.is_err() {
                println!("connect failed");
                tokio::select! {
                    _ = self.stop_notifier.notified() => {
                        println!("client recv stop notification");
                        break;
                    }
                    _ = time::sleep(time::Duration::from_secs(1)) => {
                        continue;
                    }
                }
            } else {
                println!("connect success");
                let framed = Codec::new().framed(socket.unwrap());
                let (mut writer, mut reader) = framed.split();
                let mut packet_receiver = self.packet_receiver.lock().await;
                let mut last_resend_time: u64 = 0;
                let mut last_heartbeat_time: u64 = 0;
                loop {
                    tokio::select! {
                        _ = self.stop_notifier.notified() => {
                            println!("client recv stop notification");
                            exit = true;
                            break;
                        }
                        packet = packet_receiver.recv() => {
                            println!("send a packet");
                            if let Some(packet) = packet {
                                let send_result = writer.send(packet).await;
                                if send_result.is_err() {
                                    println!("send failed");
                                }
                            } else {
                                 println!("channel receive failed");
                            }
                        }

                        packet = reader.next() => {
                            println!("recv packet from server");
                            if let Some(packet) = packet {
                                match packet {
                                    Ok(packet) => {
                                        self.process_packet(packet).await;
                                    },
                                    Err(err) => {
                                        println!("receive packet failed, err = {err}");
                                        break;
                                    }
                                }
                            } else {
                                println!("receive packet failed");
                                break;
                            }
                        }
                        _ = time::sleep(time::Duration::from_secs(1)) => {
                            let cur_time = util::get_current_timestamp();

                            if last_resend_time + RESEND_INTERVAL < cur_time {
                                last_resend_time = cur_time;
                                self.on_resend_timer().await;
                            }

                            if last_heartbeat_time + HEARTBEAT_INTERVAL < cur_time {
                                last_heartbeat_time = cur_time;
                                self.on_heartbeat_timer().await;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn process_packet(&self, packet: Packet) {
        if packet.header.push {
            self.process_push_packet(packet).await;
        } else {
            self.process_response_packet(packet).await;
        }
    }

    async fn process_response_packet(&self, packet: Packet) {
        let request_id = self.get_request_id(packet.header.cmd, packet.header.seq);
        let mut waiting_request = self.waiting_request.lock().await;
        let request = waiting_request.remove(&request_id);
        drop(waiting_request);

        if request.is_none() {
            return;
        }

        let _ = request.unwrap().notifier.send(Ok(packet));
    }

    async fn process_push_packet(&self, packet: Packet) {
        //ack
        let resp_packet = Packet {header: packet.header.clone(), body: vec![]};
        let _ = self.packet_sender.send(resp_packet).await;


        //dispatch
        let handler = self.push_handlers.get(&packet.header.cmd);
        if handler.is_none() {
            return;
        }

        let handler = handler.unwrap();
        handler(packet).await;
    }

    async fn on_resend_timer(&self) {
        let mut timeout_request_set  = HashSet::new();

        {
            let cur_time = util::get_current_timestamp();
            let mut waiting_request = self.waiting_request.lock().await;

            for (request_id, request) in waiting_request.iter_mut() {
                if request.last_send_time + request.option.resend_interval < cur_time {
                    continue;
                }

                if request.resent_times < request.option.resend_times {
                    request.resent_times += 1;
                    request.last_send_time = cur_time;
                    let _ = self.packet_sender.send(request.packet.clone()).await;
                } else {
                    timeout_request_set.insert(*request_id);
                }
            }
        }


        {
            let mut waiting_request = self.waiting_request.lock().await;

            for request_id in timeout_request_set {
                let request = waiting_request.remove(&request_id);
                if request.is_none() {
                    return;
                }

                let _ = request.unwrap().notifier.send(Err(Error::Timeout));
            }
        }
    }

    async fn on_heartbeat_timer(&self) {
        let seq = self.sequence_generator.fetch_add(1, Ordering::SeqCst);
        println!("heartbeat, seq = {seq}");
        let header = Header {
            cmd: 0,
            seq,
            device_id: self.device_id.clone(),
            push: false,
            encrypted: false,
        };
        let packet = Packet {header, body: vec![]};
        let _ = self.packet_sender.send(packet).await;
    }


    fn get_request_id(&self, cmd: u32, seq: u64) -> u128 {
        cmd as u128 >> 64 | seq as u128
    }
}