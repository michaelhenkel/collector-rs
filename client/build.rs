fn main() {
    tonic_build::configure()
    .out_dir("src/collector")
    .include_file("mod.rs")
    .type_attribute("CollectorMetrics", "#[derive(serde::Deserialize, serde::Serialize)]")
    .compile(
        &["../protos/collector.proto"],
        &["../protos"]
    )
    .unwrap();
}