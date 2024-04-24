use crate::collector::collector::{collector_server_client::CollectorServerClient, CollectorMetrics};
use log::info;
use tokio_stream::wrappers::ReceiverStream;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tonic::Request;

pub struct CollectorClient{
    address: String,
    rx: Receiver<CollectorMetrics>,
    client: Client,
}

impl CollectorClient{
    pub fn new(address: String) -> CollectorClient{
        let (tx, rx) = mpsc::channel(100);
        CollectorClient{
            address: address.clone(),
            rx,
            client: Client::new(tx, address),
        }
    }
    pub fn client(&self) -> Client{
        self.client.clone()
    }
    pub async fn run(self) -> anyhow::Result<()>{
        let mut collector_client = CollectorServerClient::connect(format!("http://{}", self.address)).await?;
        info!("Connected to server");
        let output_stream = ReceiverStream::new(self.rx);
        let request = Request::new(output_stream);
        info!("Sending metrics to collector server");
        collector_client.send_metrics(request).await?;
        info!("Client exited");
        Ok(())
    }


}

#[derive(Clone)]
pub struct Client{
    tx: Sender<CollectorMetrics>,
    address: String,
}

impl Client{
    pub fn new(tx: Sender<CollectorMetrics>, address: String) -> Client{
        Client{
            tx,
            address
        }
    }
    pub async fn send(&self, metrics: CollectorMetrics) -> anyhow::Result<()>{
        self.tx.send(metrics.clone()).await?;
        //info!("Sent metrics: {:?}", metrics);
        Ok(())
    }

    pub async fn register_metrics(&self, metrics: CollectorMetrics) -> anyhow::Result<()>{
        info!("Registering metrics: {:?}", metrics);
        let mut collector_client = CollectorServerClient::connect(format!("http://{}", self.address)).await?;
        collector_client.register_metrics(Request::new(metrics)).await?;
        Ok(())
    }
}