use std::collections::HashMap;
use std::hash::Hash;
use crate::collector::collector::CollectorMetrics;
use crate::collector_client::collector_client::Client as CollClient;
use crate::telemetry::telemetry::key_value::Value;
use crate::telemetry::telemetry::{
    Path, SubscriptionAdditionalConfig, SubscriptionMode, SubscriptionRequest
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

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct MetricsKey{
    namespace: String,
    system_id: String,
    counter_name: String,
}

#[derive(Clone, Debug)]
struct OpenConfigMetricsData{
    data: HashMap<String, u64>,
    metrics_labels: HashMap<String, String>,
}

impl OpenConfigMetricsData{
    pub fn new() -> Self{
        Self{
            data: HashMap::new(),
            metrics_labels: HashMap::new(),
        }
    }
    pub fn add(&mut self, key: String, value: u64, rate: u64){
        self.data.insert(key.clone(), value);
        self.data.insert(format!("{}_per_sec", key), rate);
    }
    pub fn add_labels(&mut self, labels: HashMap<String, String>){
        for (k,v) in labels{
            self.metrics_labels.insert(k, v);
        }
    }
}

#[derive(Clone, Debug)]
struct OpenConfigMetrics{
    prefix: String,
    metrics_data: HashMap<String,OpenConfigMetricsData>,
    prefix_labels: HashMap<String, String>,
    ts: u64,
    system_id: String,
    namespace: String,
}

impl OpenConfigMetrics{
    pub fn new(prefix: String, system_id: String, namespace: String) -> Self{
        Self{
            prefix,
            metrics_data: HashMap::new(),
            prefix_labels: HashMap::new(),
            ts: 0,
            system_id,
            namespace,
        }
    }
    pub fn add_metrics_data(&mut self, counter_key: String, counter_name: String, value: u64, labels: HashMap<String, String>, rate: u64){
        if let Some(data) = self.metrics_data.get_mut(&counter_key){
            data.add(counter_name, value, rate);
            data.add_labels(labels);
        } else {
            let mut data = OpenConfigMetricsData::new();
            data.add(counter_name, value, rate);
            data.add_labels(labels);
            self.metrics_data.insert(counter_key, data);
        }
    }

    pub fn get_metrics_data(&self, counter_name: &String) -> Option<&OpenConfigMetricsData>{
        self.metrics_data.get(counter_name)
    }

    pub fn key(&self) -> String{
        // key consists of system_id and the concatenation of all prefix_label values
        let mut key = self.system_id.clone();
        for (_,v) in &self.prefix_labels{
            key.push_str("_");
            key.push_str(&v);
        }
        key
    }
}

#[derive(Debug)]
struct OpenConfigMetricsList(Vec<OpenConfigMetrics>);

impl OpenConfigMetricsList{
    pub fn new() -> Self{
        Self(Vec::new())
    }
    pub fn add(&mut self, prefix: String, prefix_labels: HashMap<String,String>, ts: u64, system_id: String, namespace: String){
        let mut metrics = OpenConfigMetrics::new(prefix, system_id, namespace);
        metrics.prefix_labels = prefix_labels;
        metrics.ts = ts;
        self.0.push(metrics);
    }
    pub fn add_counter(&mut self, counter_key: String, counter_name: String, counter_labels: HashMap<String, String>, value: u64, rate: u64){
        if let Some(last_open_config_metrix) = self.0.last_mut(){
            let full_counter_name = format!("{}_{}", last_open_config_metrix.prefix, counter_name);
            last_open_config_metrix.add_metrics_data(counter_key, full_counter_name, value, counter_labels, rate);
        }
    }

    pub fn get_key(&self) -> String{
        if let Some(last_open_config_metrix) = self.0.last(){
            last_open_config_metrix.key()
        } else {
            "default".to_string()
        }
    }

    pub fn ts(&self) -> u64{
        self.0.last().unwrap().ts
    }
}   

impl Client{
    pub async fn subscribe_and_receive(&mut self, paths: Vec<ConfigPath>, username: String, password: String, namespace: String) -> anyhow::Result<()>{
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
        let label_re = regex::Regex::new(r"\[(.*?=.*?)\]").unwrap();
        let prefix_re = regex::Regex::new(r"\[(.*?)\]").unwrap();
        let mut prev_metrics_map: HashMap<String, OpenConfigMetrics> = HashMap::new();
        while let Some(res) = s.next().await {
            match res{
                Ok(mut x) => {
                    //info!("Received: {:#?}", x);
                    let data_path: Vec<&str> = x.path.split(":").collect();
                    if data_path.len() > 1 {
                        let p = data_path[1];
                        for path in &paths{
                            if path.path == p{
                                let mut open_config_metrics_list = OpenConfigMetricsList::new();
                                let mut ts = 0;
                                for kv in &mut x.kv{
                                    if kv.key == "__timestamp__"{
                                        ts = convert_value(&kv.value.as_ref().unwrap());
                                    } else if kv.key == "index" {
                                        continue;
                                    } else if kv.key == "__prefix__"{  
                                        if let Some(value) = &kv.value{
                                            match value{
                                                Value::StrValue(v) => {
                                                    let mut prefix_labels = HashMap::new();
                                                    for captures in label_re.captures_iter(v){
                                                        for (idx, capture) in captures.iter().enumerate(){
                                                            if idx == 0 {
                                                                continue;
                                                            }
                                                            if let Some(capture) = capture{
                                                                let capture = capture.as_str();
                                                                let key_value = capture.split("=").collect::<Vec<&str>>();
                                                                if key_value.len() == 2 {
                                                                    let key = format!("prefix_{}", key_value[0]).replace("-", "_");
                                                                    let value = key_value[1].to_string().replace("'", "").replace("-", "_");
                                                                    if !value.is_empty(){
                                                                        prefix_labels.insert(key, value);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    let prefix = prefix_re.replace_all(&v, "").to_string().replace("/", "__").replace("-", "_");
                                                    open_config_metrics_list.add(prefix.clone(), prefix_labels, ts, x.system_id.clone(), namespace.clone());
                                                },
                                                _ => {}
                                            }
                                        }
                                        //"/cos/interfaces/interface[name='et-0/0/8']/queues/queue[queue='8']/",
                                    } else {
                                        if let Some(value) = &kv.value{
                                            let converted_value = convert_value(value);
                                            let counter_name = prefix_re.replace_all(&kv.key, "").to_string().replace("/", "__");
                                            let mut labels_map = HashMap::new();
                                            let mut counter_key = "".to_string();
                                            for captures in label_re.captures_iter(&kv.key){
                                                for (idx, capture) in captures.iter().enumerate(){
                                                    if idx == 0 {
                                                        continue;
                                                    }
                                                    if let Some(capture) = capture{
                                                        let capture = capture.as_str();
                                                        let key_value = capture.split("=").collect::<Vec<&str>>();
                                                        if key_value.len() == 2 {

                                                            let key = key_value[0].replace("'", "").replace("-", "_");
                                                            if key.is_empty(){
                                                                continue;
                                                            }
                                                            
                                                            let value = key_value[1].to_string().replace("'", "");
                                                            if value.is_empty(){
                                                                continue;
                                                            }
                                                            if idx > 1 {
                                                                counter_key.push_str("_");
                                                            }
                                                            counter_key.push_str(&value);
                                                            let key = format!("counter_{}", key);
                                                            labels_map.insert(key, value);

                                                        }
                                                    }
                                                }
                                            }
                                            if counter_key.is_empty(){
                                                counter_key = open_config_metrics_list.get_key();
                                            }
                                            if !counter_key.is_empty(){
                                                let open_config_metrics_key = open_config_metrics_list.get_key();
                                                let mut rate = 0;
                                                if let Some(prev_open_config_metrics) = prev_metrics_map.get(&open_config_metrics_key){
                                                    if let Some(open_config_metrics_data) = prev_open_config_metrics.get_metrics_data(&counter_key){
                                                        if let Some(prev_data) = open_config_metrics_data.data.get(&format!("{}_{}", prev_open_config_metrics.prefix, counter_name)){
                                                            let prev_ts = prev_open_config_metrics.ts;
                                                            let actual_ts = open_config_metrics_list.ts();
                                                            let actual_data = converted_value;
                                                            let data_delta = if actual_data >= *prev_data{
                                                                actual_data - *prev_data
                                                            } else {
                                                                actual_data
                                                            };
                                                            if data_delta == 0{
                                                                rate = 0;
                                                            } else {
                                                                let ts_delta = actual_ts - prev_ts;
                                                                let per_sec_factor = ts_delta as f64 / 1_000.0;
                                                                rate = (data_delta as f64 / per_sec_factor) as u64;
                                                            }
                                                        }
                                                    }
                                                }
                                                //info!("Counter key: {}, counter name: {}, value: {}, rate: {}", counter_key, counter_name, converted_value, rate);
                                                open_config_metrics_list.add_counter(counter_key, counter_name, labels_map, converted_value, rate);
                                            }
                                        }
                                    }
                                }
                               
                                info!("\n\n\n");
                                for open_config_metrics in &open_config_metrics_list.0{
                                    let key = open_config_metrics.key();
                                    prev_metrics_map.insert(key.clone(), open_config_metrics.clone());

                                    for (_open_config_metrics_key, open_config_metrics_data) in &open_config_metrics.metrics_data{
                                        let mut labels_map = open_config_metrics.prefix_labels.clone();
                                        let mut collector_metrics = CollectorMetrics::default();
                                        for (key, value) in &open_config_metrics_data.data{
                                            labels_map.extend(open_config_metrics_data.metrics_labels.clone());
                                            collector_metrics.metrics.insert(key.clone(), *value);
                                        }
                                        labels_map.insert("namespace".to_string(), open_config_metrics.namespace.clone());
                                        labels_map.insert("system_id".to_string(), open_config_metrics.system_id.clone());
                                        collector_metrics.labels = labels_map;
                                        info!("Sending metrics: {:#?}", collector_metrics);
                                        self.collector_client.send(collector_metrics).await?;
                                    } 
                                }
                            }
                        }
                    }
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