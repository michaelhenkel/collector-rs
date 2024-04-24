use std::collections::HashMap;
use crate::{
    grpc_client::grpc_client::GrpcClient,
    scraper::scraper::Scraper,
};
use collector::collector::CollectorMetrics;
use serde::{Deserialize, Serialize};
use clap::Parser;
pub mod grpc_client;
pub mod collector;
pub mod scraper;

#[derive(Parser)]
pub struct Args{
    #[clap(short, long)]
    config: String,
}

#[derive(Serialize, Deserialize)]
pub struct Config{
    pub address: String,
    pub namespace: Option<String>,
    pub labels: Option<HashMap<String, String>>,
    pub counters: Vec<Counter>,
    pub interval: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Counter{
    pub paths: Vec<String>,
    pub labels: Option<HashMap<String, String>>,
    pub rate_keys: Option<Vec<String>>
}

#[tokio::main]
async fn main() -> anyhow::Result<()>{
    env_logger::init();
    let args = Args::parse();
    let config = std::fs::read_to_string(args.config)?;
    let config: Config = serde_yaml::from_str(&config)?;


    let mut global_labels = if let Some(global_labels) = config.labels{
        global_labels
    } else {
        HashMap::new()
    };


    if !global_labels.contains_key(&"host".to_string()){
        let host_name = hostname::get().map_err(|_| anyhow::anyhow!("Failed to get hostname"))?;
        //replace "-" with "_" in hostname
        let host_name = host_name.to_string_lossy().replace("-", "_");
        global_labels.insert("host".to_string(), host_name);
    }

    let g_client = GrpcClient::new(config.address);
    let scraper = Scraper::new(global_labels.clone(), config.counters.clone(), g_client.client(), config.interval, config.namespace.clone());

    for counter in &config.counters{
        let reg_metrics = get_metrics_metadata(counter.clone(), global_labels.clone(), config.namespace.clone())?;
        g_client.register_metrics(reg_metrics).await?;
    }

    let mut jh_list = Vec::new();
    let jh = tokio::spawn(async move {
        scraper.scrape().await
    });
    jh_list.push(jh);

    let jh = tokio::spawn(async move {
        g_client.run().await
    });
    jh_list.push(jh);
    futures::future::join_all(jh_list).await;
    Ok(())
}

pub fn get_metrics_metadata(counter: Counter, global_labels: HashMap<String, String>, namespace: Option<String>) -> anyhow::Result<CollectorMetrics>{
    let mut metrics = HashMap::new();

    let mut labels = if let Some(counter_labels) = &counter.labels{
        counter_labels.clone()
    } else {
        HashMap::new()
    };

    labels.extend(global_labels.clone());
    for path in &counter.paths{
        let files = match std::fs::read_dir(path){
            Ok(files) => files,
            Err(_e) => {
                continue;
            }
        };
        for file in files{
            let file = match file{
                Ok(file) => file,
                Err(_e) => {
                    continue;
                }
            };
            let path = file.path();
            if !path.is_file(){
                continue;
            }
            let key = file.file_name().into_string().map_err(|_| anyhow::anyhow!("Invalid file name"))?;
            metrics.insert(key.to_string(), 0);
            if let Some(rate_keys) = &counter.rate_keys{
                for rate_key in rate_keys{
                    if rate_key == &key{
                        metrics.insert(format!("{}_rate", key), 0);
                    }
                }
            }
        }
    }
    Ok(CollectorMetrics{
        labels,
        metrics,
        namespace,
    })

}
