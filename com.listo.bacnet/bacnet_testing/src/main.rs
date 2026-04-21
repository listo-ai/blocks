use std::{
    collections::HashMap,
    fs,
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use bacnet_rs::{
    app::{Apdu, MaxApduSize, MaxSegments},
    network::Npdu,
    object::{ObjectIdentifier, ObjectType, PropertyIdentifier, Segmentation},
    property::PropertyValue,
    service::{
        ConfirmedServiceChoice, IAmRequest, PropertyReference, PropertyResultValue,
        ReadAccessSpecification, ReadPropertyMultipleRequest, ReadPropertyMultipleResponse,
        UnconfirmedServiceChoice, WhoIsRequest,
    },
};
use serde::Deserialize;

const DEFAULT_CONFIG_PATH: &str = "config/bacnet-device.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(String::as_str).unwrap_or("help");

    match command {
        "whois" => run_whois(),
        "read-props" => run_read_props(args.get(2).map(String::as_str)),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command `{other}`").into()),
    }
}

fn run_whois() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_config()?;
    let socket = bind_socket(&cfg.network, cfg.network.whois_wait_ms)?;
    let devices = send_who_is(&socket, &cfg.network, &cfg.devices)?;

    println!("Who-Is results");
    println!("==============");
    println!("Configured devices:");
    for device in &cfg.devices {
        println!(
            "  {} -> {} (device id {})",
            device.name,
            device.socket_addr()?,
            device.device_id
        );
    }
    println!();

    if devices.is_empty() {
        println!("No BACnet devices answered.");
        return Ok(());
    }

    let mut found: Vec<_> = devices.values().cloned().collect();
    found.sort_by_key(|device| device.device_id);

    for device in found {
        let marker = cfg
            .devices
            .iter()
            .find(|candidate| candidate.device_id == device.device_id)
            .map(|candidate| format!(" <- configured as {}", candidate.name))
            .unwrap_or_default();

        println!(
            "device_id={} addr={} vendor={} ({}) max_apdu={} segmentation={}{}",
            device.device_id,
            device.address,
            device.vendor_name,
            device.vendor_id,
            device.max_apdu,
            device.segmentation,
            marker
        );
    }

    Ok(())
}

fn run_read_props(device_name: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_config()?;
    let client = LiveBacnetClient::new(&cfg.network)?;
    let device = select_device(&cfg, device_name)?;
    let target_addr = device.socket_addr()?;

    let specs = device
        .reads
        .iter()
        .map(read_target_to_spec)
        .collect::<Result<Vec<_>, _>>()?;

    let response = client.read_property_multiple(target_addr, &specs)?;

    println!("ReadPropertyMultiple results");
    println!("============================");
    println!(
        "Target device: {} -> {} (device id {})",
        device.name, target_addr, device.device_id
    );
    println!();

    for target in &device.reads {
        let object_id = ObjectIdentifier::new(
            parse_object_type(&target.object_type)?,
            target.object_instance,
        );

        println!("Object {:?}", object_id);

        let result = response
            .read_access_results
            .iter()
            .find(|item| item.object_identifier == object_id)
            .ok_or_else(|| format!("missing object result for {:?}", object_id))?;

        for property_name in &target.properties {
            let property_id = parse_property_identifier(property_name)?;
            let property = result
                .results
                .iter()
                .find(|item| item.property_identifier == property_id)
                .ok_or_else(|| format!("missing property {property_name} for {:?}", object_id))?;

            match &property.value {
                PropertyResultValue::Value(values) if !values.is_empty() => {
                    println!("  {property_name}: {}", format_property_values(values));
                }
                PropertyResultValue::Value(_) => {
                    println!("  {property_name}: <no values>");
                }
                PropertyResultValue::Error(error_class, error_code) => {
                    println!(
                        "  {property_name}: BACnet error class={} code={}",
                        error_class, error_code
                    );
                }
            }
        }

        println!();
    }

    Ok(())
}

fn print_help() {
    println!("bacnet-testing");
    println!();
    println!("Commands:");
    println!("  whois        Send Who-Is and print I-Am responses");
    println!("  read-props [device-name]   Read configured properties from a device");
    println!();
    println!("Config:");
    println!("  Uses BACNET_TEST_CONFIG or ./config/bacnet-device.json");
}

#[derive(Debug, Deserialize)]
struct BacnetConfig {
    #[serde(default)]
    network: NetworkConfig,
    devices: Vec<DeviceConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct NetworkConfig {
    local_bind: String,
    broadcast_addr: String,
    whois_wait_ms: u64,
    request_timeout_ms: u64,
    interface: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            local_bind: "0.0.0.0:0".into(),
            broadcast_addr: "255.255.255.255:47808".into(),
            whois_wait_ms: 5_000,
            request_timeout_ms: 5_000,
            interface: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct DeviceConfig {
    name: String,
    address: String,
    #[serde(default = "default_bacnet_port")]
    port: u16,
    device_id: u32,
    reads: Vec<ReadTarget>,
}

impl DeviceConfig {
    fn socket_addr(&self) -> Result<SocketAddr, Box<dyn std::error::Error>> {
        Ok(format!("{}:{}", self.address, self.port).parse()?)
    }
}

#[derive(Debug, Deserialize)]
struct ReadTarget {
    object_type: String,
    object_instance: u32,
    properties: Vec<String>,
}

#[derive(Debug, Clone)]
struct DiscoveredDevice {
    device_id: u32,
    address: SocketAddr,
    vendor_id: u16,
    vendor_name: String,
    max_apdu: u32,
    segmentation: Segmentation,
}

fn load_config() -> Result<BacnetConfig, Box<dyn std::error::Error>> {
    let path = config_path();
    let bytes = fs::read(&path).map_err(|err| {
        format!(
            "failed to read config at {}: {}. Copy config/bacnet-device.example.json to {} and update it.",
            path.display(),
            err,
            DEFAULT_CONFIG_PATH
        )
    })?;

    let cfg: BacnetConfig = serde_json::from_slice(&bytes)?;

    if cfg.devices.is_empty() {
        return Err("config must include at least one device".into());
    }

    if let Some(interface) = &cfg.network.interface {
        eprintln!("note: config interface is `{interface}`; current tool uses local_bind/broadcast_addr for socket selection");
    }

    Ok(cfg)
}

fn config_path() -> PathBuf {
    std::env::var("BACNET_TEST_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_CONFIG_PATH))
}

fn bind_socket(
    network: &NetworkConfig,
    timeout_ms: u64,
) -> Result<UdpSocket, Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind(&network.local_bind)?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(timeout_ms)))?;
    Ok(socket)
}

fn send_who_is(
    socket: &UdpSocket,
    network: &NetworkConfig,
    configured_devices: &[DeviceConfig],
) -> Result<HashMap<u32, DiscoveredDevice>, Box<dyn std::error::Error>> {
    let message = build_who_is_message()?;
    let broadcast_addr: SocketAddr = network.broadcast_addr.parse()?;

    socket.send_to(&message, broadcast_addr)?;
    for target in configured_devices {
        socket.send_to(&message, target.socket_addr()?)?;
    }

    let started = Instant::now();
    let timeout = Duration::from_millis(network.whois_wait_ms);
    let mut recv_buffer = [0u8; 2048];
    let mut discovered = HashMap::new();

    while started.elapsed() < timeout {
        match socket.recv_from(&mut recv_buffer) {
            Ok((len, source)) => {
                if let Some(device) = parse_i_am_response(&recv_buffer[..len], source) {
                    discovered.entry(device.device_id).or_insert(device);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(err) => return Err(err.into()),
        }
    }

    Ok(discovered)
}

fn build_who_is_message() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let who_is = WhoIsRequest::new();
    let mut service_data = Vec::new();
    who_is.encode(&mut service_data)?;

    let npdu = Npdu::global_broadcast().encode();
    let mut apdu = vec![0x10, UnconfirmedServiceChoice::WhoIs as u8];
    apdu.extend_from_slice(&service_data);

    let mut message = npdu;
    message.extend_from_slice(&apdu);

    Ok(wrap_bvlc(0x0B, message))
}

fn parse_i_am_response(data: &[u8], source: SocketAddr) -> Option<DiscoveredDevice> {
    if data.len() < 4 || data[0] != 0x81 {
        return None;
    }

    let declared_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    if declared_len != data.len() {
        return None;
    }

    let (_npdu, npdu_len) = Npdu::decode(&data[4..]).ok()?;
    let apdu = &data[4 + npdu_len..];
    if apdu.len() < 2 || apdu[0] != 0x10 || apdu[1] != UnconfirmedServiceChoice::IAm as u8 {
        return None;
    }

    let i_am = IAmRequest::decode(&apdu[2..]).ok()?;

    Some(DiscoveredDevice {
        device_id: i_am.device_identifier.instance,
        address: source,
        vendor_id: i_am.vendor_identifier,
        vendor_name: bacnet_rs::vendor::get_vendor_name(i_am.vendor_identifier)
            .unwrap_or("Unknown Vendor")
            .to_string(),
        max_apdu: i_am.max_apdu_length_accepted,
        segmentation: i_am.segmentation_supported,
    })
}

struct LiveBacnetClient {
    socket: UdpSocket,
    timeout: Duration,
}

impl LiveBacnetClient {
    fn new(network: &NetworkConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let socket = bind_socket(network, network.request_timeout_ms)?;
        Ok(Self {
            socket,
            timeout: Duration::from_millis(network.request_timeout_ms),
        })
    }

    fn read_property_multiple(
        &self,
        target_addr: SocketAddr,
        specs: &[ReadAccessSpecification],
    ) -> Result<ReadPropertyMultipleResponse, Box<dyn std::error::Error>> {
        let request = ReadPropertyMultipleRequest::new(specs.to_vec());
        let mut request_data = Vec::new();
        request.encode(&mut request_data)?;

        let response_data = self.send_confirmed_request(
            target_addr,
            1,
            ConfirmedServiceChoice::ReadPropertyMultiple,
            &request_data,
        )?;

        Ok(ReadPropertyMultipleResponse::decode(&response_data)?)
    }

    fn send_confirmed_request(
        &self,
        target_addr: SocketAddr,
        invoke_id: u8,
        service_choice: ConfirmedServiceChoice,
        service_data: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let apdu = Apdu::ConfirmedRequest {
            segmented: false,
            more_follows: false,
            segmented_response_accepted: true,
            max_segments: MaxSegments::Unspecified,
            max_response_size: MaxApduSize::Up1476,
            invoke_id,
            sequence_number: None,
            proposed_window_size: None,
            service_choice,
            service_data: service_data.to_vec(),
        };

        let mut npdu = Npdu::new();
        npdu.control.expecting_reply = true;

        let mut message = npdu.encode();
        message.extend_from_slice(&apdu.encode());

        let message = wrap_bvlc(0x0A, message);
        self.socket.send_to(&message, target_addr)?;

        let started = Instant::now();
        let mut recv_buffer = [0u8; 2048];

        while started.elapsed() < self.timeout {
            match self.socket.recv_from(&mut recv_buffer) {
                Ok((len, source)) if source == target_addr => {
                    if let Some(service_data) =
                        process_confirmed_response(&recv_buffer[..len], invoke_id)
                    {
                        return Ok(service_data);
                    }
                }
                Ok(_) => {}
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(err) => return Err(err.into()),
            }
        }

        Err(format!("BACnet request to {target_addr} timed out").into())
    }
}

fn process_confirmed_response(data: &[u8], expected_invoke_id: u8) -> Option<Vec<u8>> {
    if data.len() < 4 || data[0] != 0x81 {
        return None;
    }

    let declared_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    if declared_len != data.len() {
        return None;
    }

    let (_npdu, npdu_len) = Npdu::decode(&data[4..]).ok()?;
    let apdu = Apdu::decode(&data[4 + npdu_len..]).ok()?;

    match apdu {
        Apdu::ComplexAck {
            invoke_id,
            service_data,
            ..
        } if invoke_id == expected_invoke_id => Some(service_data),
        _ => None,
    }
}

fn wrap_bvlc(function: u8, payload: Vec<u8>) -> Vec<u8> {
    let total_len = (payload.len() + 4) as u16;
    let mut message = vec![
        0x81,
        function,
        (total_len >> 8) as u8,
        (total_len & 0xFF) as u8,
    ];
    message.extend_from_slice(&payload);
    message
}

fn read_target_to_spec(
    target: &ReadTarget,
) -> Result<ReadAccessSpecification, Box<dyn std::error::Error>> {
    let object_id = ObjectIdentifier::new(
        parse_object_type(&target.object_type)?,
        target.object_instance,
    );
    let property_refs = target
        .properties
        .iter()
        .map(|name| Ok(PropertyReference::new(parse_property_identifier(name)?)))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;

    Ok(ReadAccessSpecification::new(object_id, property_refs))
}

fn parse_object_type(value: &str) -> Result<ObjectType, Box<dyn std::error::Error>> {
    let key = normalize_key(value);
    let object_type = match key.as_str() {
        "device" => ObjectType::Device,
        "analoginput" => ObjectType::AnalogInput,
        "analogoutput" => ObjectType::AnalogOutput,
        "analogvalue" => ObjectType::AnalogValue,
        "binaryinput" => ObjectType::BinaryInput,
        "binaryoutput" => ObjectType::BinaryOutput,
        "binaryvalue" => ObjectType::BinaryValue,
        "multistateinput" => ObjectType::MultiStateInput,
        "multistateoutput" => ObjectType::MultiStateOutput,
        "multistatevalue" => ObjectType::MultiStateValue,
        other => return Err(format!("unsupported object_type `{other}` in config").into()),
    };

    Ok(object_type)
}

fn parse_property_identifier(
    value: &str,
) -> Result<PropertyIdentifier, Box<dyn std::error::Error>> {
    let property = match normalize_key(value).as_str() {
        "objectname" => PropertyIdentifier::ObjectName,
        "description" => PropertyIdentifier::Description,
        "presentvalue" => PropertyIdentifier::PresentValue,
        "statusflags" => PropertyIdentifier::StatusFlags,
        "units" => PropertyIdentifier::Units,
        "objectlist" => PropertyIdentifier::ObjectList,
        "modelname" => PropertyIdentifier::ModelName,
        "firmwarerevision" => PropertyIdentifier::FirmwareRevision,
        "vendorname" => PropertyIdentifier::VendorName,
        "vendoridentifier" => PropertyIdentifier::VendorIdentifier,
        "objectidentifier" => PropertyIdentifier::ObjectIdentifier,
        other => return Err(format!("unsupported property `{other}` in config").into()),
    };

    Ok(property)
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn format_property_values(values: &[PropertyValue]) -> String {
    values
        .iter()
        .map(|value| format!("{value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn default_bacnet_port() -> u16 {
    47_808
}

fn select_device<'a>(
    cfg: &'a BacnetConfig,
    requested_name: Option<&str>,
) -> Result<&'a DeviceConfig, Box<dyn std::error::Error>> {
    match requested_name {
        Some(name) => cfg
            .devices
            .iter()
            .find(|device| device.name == name)
            .ok_or_else(|| {
                format!(
                    "unknown device `{name}`. Available devices: {}",
                    cfg.devices
                        .iter()
                        .map(|device| device.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .into()
            }),
        None => {
            if cfg.devices.len() == 1 {
                Ok(&cfg.devices[0])
            } else {
                Err(format!(
                    "multiple devices configured. Choose one: {}",
                    cfg.devices
                        .iter()
                        .map(|device| device.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .into())
            }
        }
    }
}
