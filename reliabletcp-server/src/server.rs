use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime;
use tokio::sync::{Mutex, Notify};
use tokio::time;
use reliabletcp_common::{util, Header, PacketHandler};
use crate::connection::Connection;


const HEARTBEAT_INTERVAL : u64 = 60 * 1000;

pub struct TcpServer {
    runtime: &'static runtime::Runtime,
    ip: String,
    port: u16,
    handlers: Arc<HashMap<u32, PacketHandler>>,

    stop_notifier: Arc<Notify>,
    started: AtomicBool,

    connections: Mutex<Vec<Arc<Connection>>>,
}

impl TcpServer {
    pub fn new(runtime: &'static runtime::Runtime, ip: &str, port: u16, handlers: Arc<HashMap<u32, PacketHandler>>) -> Arc<Self> {
        Arc::new(TcpServer {
            runtime,
            ip: ip.to_string(),
            port,
            handlers,
            stop_notifier: Arc::new(Notify::new()),
            started: AtomicBool::new(false),
            connections: Mutex::new(Vec::new()),
        })
    }

    pub async fn start(self: Arc<Self>) {
        if self.started.load(Ordering::SeqCst) {
            return;
        }

        let self_clone = self.clone();
        tokio::spawn(async move {
            self.started.store(true, Ordering::SeqCst);
            println!("server loop started");
            self_clone.run_loop().await;
            println!("server loop stopped");
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

    pub async fn response(self: Arc<Self>, req_header: Header, body: Vec<u8>) -> bool {
        let connections = self.connections.lock().await;
        for conn in connections.iter() {
            if conn.device_id().await == req_header.device_id.clone() {
                return conn.clone().response(req_header, body).await;
            }
        }
        return false;
    }

    pub async fn push(self: Arc<Self>, device_id: String, cmd: u32, encrypted: bool, body: Vec<u8>) -> bool {
        let connections = self.connections.lock().await;
        for conn in connections.iter() {
            if conn.device_id().await == device_id {
                return conn.clone().push(cmd, encrypted, body).await;
            }
        }

        return false;
    }

    async fn run_loop(self: Arc<Self>) {
        let addr = format!("{}:{}", self.ip, self.port);

        let listener = TcpListener::bind(&addr).await;
        if listener.is_err() {
            return;
        }
        let listener = listener.unwrap();

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    if accept_result.is_err() {
                        break;
                    }
                    let (socket, _) = accept_result.unwrap();
                    self.add_connection(socket).await;
                }
                _ = self.stop_notifier.notified() => {
                    println!("server recv stop notification");
                    break;
                }
                _ = time::sleep(time::Duration::from_secs(1)) => {
                    self.on_timer().await;
                }
            }
        }

        println!("clear connection");
        self.connections.lock().await.clear();
    }

    async fn add_connection(&self, socket: TcpStream) {
        let conn = Connection::new(self.runtime, socket, self.handlers.clone());
        self.connections.lock().await.push(conn);
    }

    async fn on_timer(&self) {
        let mut connections = self.connections.lock().await;
        let cur_timestamp = util::get_current_timestamp();

        connections.retain(|conn| cur_timestamp - conn.last_received_packet_time() > HEARTBEAT_INTERVAL);
    }
}