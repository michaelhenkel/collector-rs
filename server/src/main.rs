use crate::grpc_server::grpc_server::GrpcServer;
use clap::Parser;
use log::info;
use prometheus::prometheus::Prometheus;
use tokio::signal;

pub mod grpc_server;
pub mod collector;
pub mod prometheus;

#[derive(Parser)]
pub struct Args{
    #[clap(short, long, default_value = "0.0.0.0:50055")]
    grpc_address: String,
    #[clap(short, long, default_value = "0.0.0.0:50056")]
    prometheus_address: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()>{
    env_logger::init();
    let args = Args::parse();

    let prom_server = Prometheus::new(args.prometheus_address);

    let g_server = GrpcServer::new(args.grpc_address, prom_server.client());

    let mut jh_list = Vec::new();

    let jh = tokio::spawn(async move {
        prom_server.web_server().await
    });
    jh_list.push(jh);

    let jh = tokio::spawn(async move {
        g_server.run().await
    });
    jh_list.push(jh);

    futures::future::join_all(jh_list).await;
    signal::ctrl_c().await?;
    info!("Exiting...");
    Ok(())
}
