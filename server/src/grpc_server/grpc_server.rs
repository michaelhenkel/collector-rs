use crate::collector::collector::{
    collector_server_server::{CollectorServer, CollectorServerServer},
    CollectorMetrics, Reply,
};
use crate::prometheus::prometheus::Client as PrometheusClient;
use log::info;
use tonic::{transport::Server, Request, Response, Status, Streaming};
use tokio_stream::StreamExt;

#[derive(Clone)]
pub struct GrpcServer{
    address: String,
    prometheus_client: PrometheusClient
}

impl GrpcServer {
    pub fn new(address: String, prometheus_client: PrometheusClient) -> GrpcServer {
        GrpcServer {
            address,
            prometheus_client
        }
    }

    pub async fn run(&self) -> anyhow::Result<()>{
        info!("Server listening on {}", self.address);
        Server::builder()
            .add_service(CollectorServerServer::new(self.clone()))
            .serve(self.address.parse().unwrap())
            .await?;
        Ok(())
    }
}


#[tonic::async_trait]
impl CollectorServer for GrpcServer {
    async fn send_metrics(
        &self,
        request: Request<Streaming<CollectorMetrics>>,
    ) -> Result<Response<Reply>, Status> {
        info!("Received metrics request");
        let mut stream = request.into_inner();
        while let Some(metrics) = stream.next().await {
            let metrics = metrics?;
            self.prometheus_client.send_metrics(metrics).await.map_err(|e| {
                Status::internal(format!("Failed to send metrics: {}", e))
            })?;
        }
        Ok(Response::new(Reply::default()))
    }
    async fn register_metrics(
        &self,
        request: Request<CollectorMetrics>,
    ) -> Result<Response<Reply>, Status> {
        info!("Received register request");
        let metrics = request.into_inner();
        self.prometheus_client.register(metrics).await.map_err(|e| {
            Status::internal(format!("Failed to register metrics: {}", e))
        })?;
        Ok(Response::new(Reply::default()))
    }
}