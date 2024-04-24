
use log::{error,info};
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;
use crate::collector::collector::{collector_server_client::CollectorServerClient, CollectorMetrics};

pub struct GrpcClient{
    address: String,
    client: Client,
    rx: tokio::sync::mpsc::Receiver<CollectorMetrics>
}

#[derive(Clone)]
pub struct Client{
    tx: tokio::sync::mpsc::Sender<CollectorMetrics>,
}

impl Client{
    pub fn new(tx: tokio::sync::mpsc::Sender<CollectorMetrics>) -> Client{
        Client{
            tx,
        }
    }
    pub async fn send(&self, metrics: CollectorMetrics) -> anyhow::Result<()>{
        self.tx.send(metrics.clone()).await?;
        //info!("Sent metrics: {:?}", metrics);
        Ok(())
    }
}

impl GrpcClient {
    pub fn new(address: String) -> GrpcClient {
        let (tx, rx) = tokio::sync::mpsc::channel(10000);
        GrpcClient {
            address,
            client: Client::new(tx),
            rx,
        }
    }

    pub fn client(&self) -> Client{
        self.client.clone()
    }

    pub async fn register_metrics(&self, metrics: CollectorMetrics) -> anyhow::Result<()>{
        let mut collector_client = CollectorServerClient::connect(format!("http://{}", self.address)).await?;
        collector_client.register_metrics(Request::new(metrics)).await?;
        Ok(())
    }

    pub async fn run(self) -> anyhow::Result<()>{
        let mut collector_client = CollectorServerClient::connect(format!("http://{}", self.address)).await?;
        info!("Connected to server");
        let output_stream = ReceiverStream::new(self.rx);
        let request = Request::new(output_stream);
        info!("Sending metrics to collector server");
        collector_client.send_metrics(request).await?;
        error!("Client exited");
        Ok(())
    }
}