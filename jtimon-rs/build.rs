fn main() {
    tonic_build::configure()
    .out_dir("src/jnx")
    .include_file("mod.rs")
    //.type_attribute("CollectorMetrics", "#[derive(serde::Deserialize, serde::Serialize)]")
    .compile(
        &["protos/jnx/jnx_authentication_service.proto",
        "protos/jnx/jnx_common_base_types.proto",
        "protos/jnx/jnx_management_service.proto"],
        &["protos/jnx"]
    )
    .unwrap();

    tonic_build::configure()
    .out_dir("src/gnmi")
    .include_file("mod.rs")
    .compile(
        &["protos/gnmi/gnmi.proto",
        "protos/gnmi/gnmi_ext.proto"],
        &["protos/gnmi"]
    )
    .unwrap();

    tonic_build::configure()
    .out_dir("src/gnmi_jnpr")
    .include_file("mod.rs")
    .compile(
        &["protos/gnmi_jnpr/gnmi_jnpr_hdr_ext.proto",
        "protos/gnmi_jnpr/gnmi_jnpr_hdr.proto"],
        &["protos/gnmi_jnpr"]
    )
    .unwrap();


    tonic_build::configure()
    .out_dir("src/telemetry")
    .include_file("mod.rs")
    .compile(
        &["protos/telemetry/telemetry.proto"],
        &["protos/telemetry"]
    )
    .unwrap();

    tonic_build::configure()
    .out_dir("src/collector")
    .include_file("mod.rs")
    .compile(
        &["../protos/collector.proto"],
        &["../protos"]
    )
    .unwrap();
}