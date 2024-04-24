use std::collections::HashMap;
use log::info;
use crate::{collector::collector::CollectorMetrics, grpc_client::grpc_client::Client, Counter};

pub struct Scraper{
    global_labels: HashMap<String, String>,
    namespace: Option<String>,
    counters: Vec<Counter>,
    client: Client,
    interval: u64,
}

impl Scraper{
    pub fn new(global_labels: HashMap<String,String>, counters: Vec<Counter>, client: Client, interval: u64, namespace: Option<String>) -> Scraper{
        Scraper{
            global_labels,
            namespace,
            counters,
            client,
            interval,
        }
    }
    pub async fn scrape(&self) -> anyhow::Result<()>{
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(self.interval));
        info!("Starting scraper at interval: {} ms", self.interval);
        let mut rate_map = HashMap::new();
        loop{
            interval.tick().await;
            info!("Scraping counters: {:?}", self.counters);
            for counter in &self.counters{
                let mut metrics = HashMap::new();
                let mut labels = if let Some(counter_labels) = &counter.labels{
                    counter_labels.clone()
                } else {
                    HashMap::new()
                };
                labels.extend(self.global_labels.clone());
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
                        let value = read_counter(path.to_str().ok_or(anyhow::anyhow!("Invalid path"))?);
                        metrics.insert(key.to_string(), value);
                        if let Some(rate_keys) = &counter.rate_keys{
                            for rate_key in rate_keys{
                                if rate_key == &key{
                                    let prev_rate = rate_map.get(&key).unwrap_or(&0);
                                    let rate = if value >= *prev_rate{
                                        value - *prev_rate
                                    } else {
                                        value
                                    };
                                    metrics.insert(format!("{}_rate", key), rate);
                                    rate_map.insert(key.clone(), value);
                                }
                            }

                        }
                    }
                }
                info!("Scraped metrics: {:?}", metrics);
                let collector_metrics = CollectorMetrics{
                    labels,
                    metrics,
                    namespace: self.namespace.clone(),
                };
                self.client.send(collector_metrics).await?;

            }

        }
    }
}

fn read_counter(path: &str) -> u64
{
    let v = match std::fs::read_to_string(path){
        Ok(v) => v,
        Err(_e) => {
            return 0;
        }
    };
    match v.trim().parse::<u64>(){
        Ok(v) => v,
        Err(_e) => {
            0
        }
    }
}