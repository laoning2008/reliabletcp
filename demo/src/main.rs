use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use reliabletcp_server::TcpServer;
use reliabletcp_client::TcpClient;
use reliabletcp_common::PacketHandler;
use reliabletcp_common::Header;
use reliabletcp_common::Packet;
use reliabletcp_common::SendOption;
use tokio::runtime;
#[macro_use]
extern crate lazy_static;



lazy_static! {
    static ref rt: runtime::Runtime = runtime::Runtime::new().unwrap();

}

fn main() {
    log::set_max_level(log::LevelFilter::Info);

    let handlers: Arc<HashMap<u32, PacketHandler>> = Arc::new(HashMap::new());
    let server = TcpServer::new(&rt, "127.0.0.1", 8090, handlers);
    let server_ctrl_c = server.clone();

    let digest = md5::compute(b"device_id");
    let device_id = format!("{:x}", digest);
    println!("device_id = {}", device_id);

    let push_handlers: Arc<HashMap<u32, PacketHandler>> = Arc::new(HashMap::new());
    let client = TcpClient::new(&rt, "127.0.0.1", 8090, device_id, push_handlers);
    let client_request = client.clone();
    let client_ctrl_c = client.clone();


    rt.block_on(async {
        server.start().await;
        client.start().await;

        rt.spawn(async move {
            tokio::signal::ctrl_c().await;
            println!("ctrl-c received!");
            server_ctrl_c.stop().await;
            client_ctrl_c.stop().await;
        });

        loop {
            tokio::time::sleep(Duration::from_millis(2)).await;
            client_request.clone().request(1, false, vec![], SendOption::default()).await;
        }
    });


    println!("exit!")
}