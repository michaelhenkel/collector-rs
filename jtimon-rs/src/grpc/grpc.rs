use std::collections::{HashMap, HashSet};

use crate::collector::collector::CollectorMetrics;
use crate::collector_client::collector_client::Client as CollClient;
use crate::telemetry::telemetry::key_value::Value;
use crate::telemetry::telemetry::{
    OpenConfigData, Path, SubscriptionAdditionalConfig, SubscriptionMode, SubscriptionRequest
};
use futures::StreamExt;
use tonic::Request as GrpcRequest;
use crate::Path as ConfigPath;
use crate::jnx::jnx::jet::authentication as junos_auth;
use crate::Tls;
use crate::telemetry::telemetry::open_config_telemetry_client::OpenConfigTelemetryClient;
use log::error;
use tonic::metadata::MetadataMap;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use log::info;
pub struct Grpc{
    client: Client,
}

impl Grpc{
    pub async fn new(address: String, tls: Tls, username: String, password: String, collector_client: CollClient) -> anyhow::Result<Self>{
        let mut map = MetadataMap::new();
        let ca = std::fs::read(tls.ca_file).unwrap();
        let crt = std::fs::read(tls.cert_file).unwrap();
        let key = std::fs::read(tls.key_file).unwrap();
        map.insert("client-id", "cnm".parse().unwrap());
        let identity = tonic::transport::Identity::from_pem(crt, key);
        let tls = ClientTlsConfig::new()
            .domain_name(tls.server_name)
            .identity(identity)
            .ca_certificate(Certificate::from_pem(ca));

        let ep_address = format!("https://{}",address);
        info!("Connecting to {}", ep_address);
        let channel = Channel::from_shared(ep_address.clone())?
            .tls_config(tls)?
            .connect()
            .await?;
        info!("Connected to {}", ep_address);

        let login_request = junos_auth::LoginRequest{
            username,
            password,
            group_id: "cnm".to_string(),
            client_id: "cnm".to_string(),
        };

        let login_response = match junos_auth::authentication_client::AuthenticationClient::new(channel.clone()).login(login_request).await{
            Ok(res) => {
                res
            },
            Err(e) => {
                error!("Failed to login: {:?}", e);
                return Err(e.into())
            }
        };
        info!("login response: {:#?}", login_response.into_inner());
        let client = OpenConfigTelemetryClient::new(channel);
        let client = Client{junos_client: client, collector_client};
        Ok(Self{client})
    }
    pub fn client(&self) -> Client{
        self.client.clone()
    }
}

#[derive(Clone)]
pub struct Client{
    junos_client: OpenConfigTelemetryClient<tonic::transport::Channel>,
    collector_client: CollClient,
}

impl Client{
    pub async fn subscribe_and_receive(&mut self, paths: Vec<ConfigPath>, username: String, password: String, namespace: String, device: String) -> anyhow::Result<()>{
        let mut sub_req = SubscriptionRequest::default();
        let mut add_config = SubscriptionAdditionalConfig::default();
        add_config.set_mode(SubscriptionMode::LongLived);
        add_config.need_eos = true;
        sub_req.additional_config = Some(add_config);
        
        let mut path_list: Vec<Path> = Vec::new();
        for p in &paths{
            let mut path = Path::default();
            path.path = p.path.clone();
            path.sample_frequency = p.freq;
            path_list.push(path);
        }
        sub_req.path_list = path_list;
        let mut req = GrpcRequest::new(sub_req);
        req.metadata_mut().insert("client-id", "cnm".parse().unwrap());
        req.metadata_mut().insert("username", username.parse().unwrap());
        req.metadata_mut().insert("password", password.parse().unwrap()); 
        let res = self.junos_client.telemetry_subscribe(req).await?;
        let mut s = res.into_inner();
        let mut registered = false;
        let mut rate_map: HashMap<String, (u64, u64)> = HashMap::new();
        let now = tokio::time::Instant::now();
        while let Some(res) = s.next().await {
            match res{
                Ok(mut x) => {
                    let mut metrics_map = HashMap::new();
                    let mut label_map = HashMap::new();
                    //info!("got request: {:#?}, elapsed: {}, sync: {:#?}", x.path, now.elapsed().as_millis(), x);
                    let data_path: Vec<&str> = x.path.split(":").collect();
                    if data_path.len() > 1 {
                        let p = data_path[1];
                        for path in &paths{
                            if path.path == p{
                                if let Some(regexps) = &path.label_regexps{
                                    for (label_key, regexp) in regexps{
                                        let re = regex::Regex::new(regexp).unwrap();
                                        for kv in &mut x.kv{
                                            if let Some(captures) = re.captures(&kv.key){
                                                if let Some(capture) = captures.get(1){
                                                    if let Some(value) = &kv.value{
                                                        let key = format!("{}_{}_{}", namespace.clone(), label_key.to_string(), capture.as_str());
                                                        metrics_map.insert(key, convert_value(value));
                                                        //label_map.insert(label_key.to_string(), capture.as_str().to_string());

                                                    }
                                                    //info!("Capture counters: {}", capture.as_str());
                                                }
                                            }
                                        }
                                    }
                                }
                                if let Some(regexps) = &path.rate_keys{
                                    for (rate_key, regexp) in regexps{
                                        let re = regex::Regex::new(regexp).unwrap();
                                        for kv in &mut x.kv{
                                            if let Some(captures) = re.captures(&kv.key){
                                                if let Some(capture) = captures.get(1){
                                                    if let Some(value) = &kv.value{
                                                        let cur_ts = x.timestamp;
                                                        let key = format!("{}_{}_{}", namespace.clone(), rate_key, capture.as_str());
                                                        let (prev_rate, prev_ts) = rate_map.get(&key).unwrap_or(&(0, 0));
                                                        let value = convert_value(value);
                                                        let rate = if value >= *prev_rate{
                                                            value - *prev_rate
                                                        } else {
                                                            value
                                                        };

                                                        let delta_ts = cur_ts - *prev_ts;

                                                        // compute per sec
                                                        let per_sec_factor = delta_ts as f64 / 1_000.0;
                                                        let rate_per_sec = rate as f64 / per_sec_factor;
                                                        if rate > 0 {
                                                            info!("cur_value {}, prev_value {}, rate {}, cur_ts {}, prev_ts {}, delta_ts {}, rate_per_sec {}", value, *prev_rate, rate, cur_ts, *prev_ts, delta_ts, rate_per_sec);
                                                        }

                                                        metrics_map.insert(key.clone(), rate_per_sec as u64);
                                                        rate_map.insert(key, (value, cur_ts));
                                                        //label_map.insert(rate_key.to_string(), capture.as_str().to_string());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    let mut collector_metrics = CollectorMetrics::default();
                    collector_metrics.metrics = metrics_map;
                    label_map.insert("namespace".to_string(), namespace.clone());
                    collector_metrics.labels = label_map;
                    collector_metrics.namespace = Some(namespace.clone());
                    if !registered{
                        self.collector_client.register_metrics(collector_metrics.clone()).await?;
                        registered = true;
                    }
                    self.collector_client.send(collector_metrics).await?;
                },
                Err(e) => {
                    error!("Failed to receive: {:?}", e);
                }
            }
        };
        info!("Done");
        Ok(())
    }
}

fn convert_value(value: &Value) -> u64{
    match value{
        Value::DoubleValue(v) => {
            *v as u64
        },
        Value::FloatValue(v) => {
            *v as u64
        },
        Value::UintValue(v) => {
            *v
        },
        Value::IntValue(v) => {
            *v as u64
        },
        _ => {
            0
        }
    }
}