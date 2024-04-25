use clap::Parser;
use grpc::grpc::Grpc;
use collector_client::collector_client::CollectorClient;

pub mod jnx;
pub mod grpc;
pub mod gnmi;
pub mod gnmi_jnpr;
pub mod telemetry;
pub mod collector;
pub mod collector_client;

#[derive(Parser)]
pub struct Args{
    #[clap(short, long)]
    config: String,
}

#[derive(serde::Deserialize)]
struct Config{
    devices: Vec<Device>,
    collector: Collector,
}

#[derive(serde::Deserialize)]
struct Device{
    address: String,
    user: String,
    password: String,
    tls: Tls,
    paths: Vec<Path>,
    namespace: String,
}

#[derive(serde::Deserialize)]
struct Collector{
    address: String,
}

#[derive(serde::Deserialize)]
pub struct Tls{
    pub cert_file: String,
    pub key_file: String,
    pub ca_file: String,
    pub server_name: String,
}

#[derive(serde::Deserialize, Debug)]
pub struct Path{
    path: String,
    freq: u32,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    let config = std::fs::read_to_string(args.config).unwrap();
    let config: Config = serde_yaml::from_str(&config).unwrap();
    let mut jh_list = Vec::new();
    let col_client = CollectorClient::new(config.collector.address);
    let col_client_client = col_client.client();
    let jh = tokio::spawn(async move {
        if let Err(e) = col_client.run().await{
            log::error!("Failed to run collector: {:?}", e);
        }
    });
    jh_list.push(jh);
    for device in config.devices{
        let grpc = Grpc::new(device.address.clone(), device.tls, device.user.clone(), device.password.clone(), col_client_client.clone()).await.unwrap();
        let mut client = grpc.client();
        let jh = tokio::spawn(async move{
            if let Err(e) = client.subscribe_and_receive(device.paths, device.user, device.password, device.namespace).await{
                log::error!("Failed to subscribe: {:?}", e);
            };
        });
        jh_list.push(jh);
    }
    

    futures::future::join_all(jh_list).await;
    println!("Hello, world! xx");
}
