use std::{collections::HashMap, pin::Pin, sync::Arc};
use actix_web::{get, App, HttpServer, Responder};
use actix_web_prom::PrometheusMetricsBuilder;
use log::info;
use prometheus::{GaugeVec, Registry};
use tokio::sync::RwLock;

use crate::collector::collector::CollectorMetrics;

#[get("/")]
async fn index() -> impl Responder {
    "Hello, World!"
}

pub struct Prometheus{
    rx: Arc<RwLock<tokio::sync::mpsc::Receiver<WebServerCommand>>>,
    client: Client,
    address: String,
}

impl Prometheus{
    pub fn new(address: String) -> Prometheus{
        let (tx, rx) = tokio::sync::mpsc::channel(10000);
        Prometheus{
            rx: Arc::new(RwLock::new(rx)),
            client: Client::new(tx),
            address,
        }
    }

    pub fn client(&self) -> Client{
        self.client.clone()
    }

    pub async fn run(&self) -> anyhow::Result<()>{

        Ok(())
    }

    pub async fn web_server(&self) -> anyhow::Result<()> {
        let mut prometheus = PrometheusMetricsBuilder::new("api")
        .endpoint("/metrics")
        .build()
        .unwrap();

        let mut gauge_map: Arc<HashMap<String, GaugeVec>> = Arc::new(HashMap::new());
        let registry = Registry::new();
        let rx = self.rx.clone();
        let mut web_server_handle = None;

        while let Some(command) = rx.write().await.recv().await{
            match command{
                WebServerCommand::Start => {
                    info!("Starting web server");
                    let prometheus = prometheus.clone();
                    let address = self.address.clone();
                    let handle: tokio::task::JoinHandle<Result<(), anyhow::Error>> = tokio::spawn(async move {
                        let server = HttpServer::new(move || {
                            App::new()
                            .wrap(prometheus.clone())
                            .service(index)
                        })
                        .bind(address.clone())?
                        .shutdown_timeout(1);
                        server.run().await?;
                        Ok(())
                    });
                    web_server_handle = Some(handle);
                },
                WebServerCommand::Stop => {
                    info!("Stopping web server");
                    if let Some(ref web_server_handle) = web_server_handle{
                        web_server_handle.abort();
                    }
                },
                WebServerCommand::Register(collector_metrics) => {
                    info!("Registering metrics");
                    let gm = setup_metrics(collector_metrics);
                    let mut new_gauge_map = HashMap::new();
                    for (k, v) in &gm{
                        if !gauge_map.contains_key(k){
                            match registry.register(Box::new(v.clone())){
                                Ok(_) => {},
                                Err(e) => {
                                    if e.to_string().contains("Duplicate metrics collector registration attempted"){
                                        continue;
                                    }
                                    info!("Failed to register metric:{} {}", k, e);
                                }
                            }
                            new_gauge_map.insert(k.clone(), v.clone());
                        }
                    }
                    if new_gauge_map.len() > 0{
                        for (k,v) in gauge_map.iter(){
                            new_gauge_map.insert(k.clone(), v.clone());
                        }
                        gauge_map = Arc::new(new_gauge_map);
                        prometheus.registry = registry.clone();
                        self.client.stop().await?;
                        self.client.start().await?;
                    }
                },
                WebServerCommand::SendMetrics(metrics) => {
                    let mut vals = Vec::new();
                    let mut sorted_list = metrics.labels.iter().collect::<Vec<_>>();
                    sorted_list.sort_by(|a, b| a.0.cmp(b.0));

                    for (_, label_value) in sorted_list {
                        vals.push(label_value.clone());
                    }

                    let vals: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
                    let slice_vals = vals.as_slice();
                    let mut not_found = false;
                    for (k, v) in metrics.clone().metrics {
                        if let Some(gauge) = gauge_map.get(&k){
                            Pin::new(gauge).with_label_values(&slice_vals).set(v as f64);
                        } else {
                            not_found = true;
                            
                            info!("Metric not found: {}", k);
                        }
                    }
                    if not_found{
                        self.client.register(metrics.clone()).await?;
                    }
                },
            }
        }

        let server = HttpServer::new(move || {
            App::new()
            .wrap(prometheus.clone())
            .service(index)
        })
        .bind(self.address.clone())?
        .shutdown_timeout(1);
        
        server.run().await?;

        

        Ok(())
    }
}

fn setup_metrics(metrics: CollectorMetrics) -> HashMap<String, GaugeVec> {
    let mut gauge_map = HashMap::new();
    let mut vals = Vec::with_capacity(metrics.labels.len());

    // sort the hashmap metrics.labels by key
    let mut sorted_list = metrics.labels.iter().collect::<Vec<_>>();
    sorted_list.sort_by(|a, b| a.0.cmp(b.0));

    for (label_key,_) in sorted_list {
        vals.push(label_key.clone());
    }

    let vals: Vec<&str> = vals.iter().map(|s| s.as_str()).collect();
    let slice_vals = vals.as_slice();
    for k in metrics.metrics.keys(){
        let opts = prometheus::Opts::new(k.clone(), k.clone());
        let gauge = GaugeVec::new(
            if let Some(namespace) = &metrics.namespace{
                opts.namespace(namespace.clone())
            } else {
                opts
            },
            slice_vals,
        ).unwrap();
        gauge_map.insert(k.clone(), gauge);
    }
    gauge_map
}


enum WebServerCommand{
    Start,
    Stop,
    SendMetrics(CollectorMetrics),
    Register(CollectorMetrics)
}

#[derive(Clone)]
pub struct Client{
    tx: tokio::sync::mpsc::Sender<WebServerCommand>,
}

impl Client{
    fn new(tx: tokio::sync::mpsc::Sender<WebServerCommand>) -> Client{
        Client{
            tx,
        }
    }

    pub async fn register(&self, metrics: CollectorMetrics) -> anyhow::Result<()>{
        self.tx.send(WebServerCommand::Register(metrics)).await?;
        Ok(())
    }

    pub async fn send_metrics(&self, metrics: CollectorMetrics) -> anyhow::Result<()>{
        self.tx.send(WebServerCommand::SendMetrics(metrics)).await?;
        Ok(())
    }

    pub async fn start(&self) -> anyhow::Result<()>{
        self.tx.send(WebServerCommand::Start).await?;
        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<()>{
        self.tx.send(WebServerCommand::Stop).await?;
        Ok(())
    }
}