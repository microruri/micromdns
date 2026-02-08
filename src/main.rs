use if_addrs::get_if_addrs;
use libmdns::Responder;
use log::{debug, error, info, warn};
use std::collections::BTreeSet;
use std::env;
use std::io;
use std::net::IpAddr;
use std::time::Duration;

const DEFAULT_POLL_SECONDS: u64 = 3;

#[derive(Debug, Clone)]
struct CliOptions {
    name: String,
    interfaces: Vec<String>,
}

#[derive(Debug, Clone)]
enum InterfaceFilter {
    All,
    Only(BTreeSet<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InterfaceSnapshot {
    name: String,
    ip: IpAddr,
    index: Option<u32>,
}

impl InterfaceFilter {
    fn from_values(values: &[String]) -> Self {
        if values.is_empty() {
            return Self::All;
        }

        let mut selected = BTreeSet::new();
        for value in values {
            for item in value.split(',') {
                let iface = item.trim();
                if iface.is_empty() {
                    continue;
                }
                if iface == "*" {
                    return Self::All;
                }
                selected.insert(iface.to_owned());
            }
        }

        if selected.is_empty() {
            Self::All
        } else {
            Self::Only(selected)
        }
    }

    fn matches(&self, iface_name: &str) -> bool {
        match self {
            Self::All => true,
            Self::Only(only) => only.contains(iface_name),
        }
    }

    fn as_log_value(&self) -> String {
        match self {
            Self::All => "*".to_owned(),
            Self::Only(only) => only.iter().cloned().collect::<Vec<_>>().join(","),
        }
    }
}

fn print_usage(program: &str) {
    println!(
        "Usage:\n  {program} --name <name> [--interface <iface> ...]\n  {program} <name> [--interface <iface> ...]\n\nOptions:\n  -n, --name <name>           Host name, resolves as <name>.local\n  -i, --interface <iface>     Interface name, repeatable. Default is '*' (all)\n  -h, --help                  Show this help"
    );
}

fn parse_args() -> Result<CliOptions, String> {
    let args: Vec<String> = env::args().collect();
    let program = args.first().cloned().unwrap_or_else(|| "mdnsd".to_owned());

    let mut name: Option<String> = None;
    let mut interfaces = Vec::new();
    let mut positional = Vec::new();
    let mut i = 1usize;

    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage(&program);
                std::process::exit(0);
            }
            "-n" | "--name" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing value after --name/-n".to_owned());
                }
                name = Some(args[i].clone());
            }
            "-i" | "--interface" => {
                i += 1;
                if i >= args.len() {
                    return Err("missing value after --interface/-i".to_owned());
                }
                interfaces.push(args[i].clone());
            }
            _ if arg.starts_with("--name=") => {
                let value = arg.trim_start_matches("--name=").trim();
                if value.is_empty() {
                    return Err("empty value in --name=<value>".to_owned());
                }
                name = Some(value.to_owned());
            }
            _ if arg.starts_with("--interface=") => {
                let value = arg.trim_start_matches("--interface=").trim();
                if value.is_empty() {
                    return Err("empty value in --interface=<value>".to_owned());
                }
                interfaces.push(value.to_owned());
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ => positional.push(arg.clone()),
        }
        i += 1;
    }

    if name.is_none() {
        if positional.len() == 1 {
            name = positional.pop();
        } else if positional.len() > 1 {
            return Err(format!(
                "too many positional arguments: {}",
                positional.join(" ")
            ));
        }
    } else if !positional.is_empty() {
        return Err(format!(
            "unexpected positional arguments: {}",
            positional.join(" ")
        ));
    }

    let name = match name {
        Some(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Err("name cannot be empty".to_owned());
            }
            trimmed.to_owned()
        }
        None => {
            return Err("missing required name. use --name <name> or positional <name>".to_owned());
        }
    };

    if interfaces.is_empty() {
        interfaces.push("*".to_owned());
    }

    Ok(CliOptions { name, interfaces })
}

fn collect_snapshot(filter: &InterfaceFilter) -> io::Result<Vec<InterfaceSnapshot>> {
    let mut result = Vec::new();

    for iface in get_if_addrs()? {
        if iface.is_loopback() {
            continue;
        }
        if !filter.matches(&iface.name) {
            continue;
        }

        let ip = iface.ip();
        let index = iface.index;
        let name = iface.name;
        result.push(InterfaceSnapshot { name, ip, index });
    }

    result.sort();
    Ok(result)
}

fn collect_missing_interfaces(filter: &InterfaceFilter) -> io::Result<Vec<String>> {
    let InterfaceFilter::Only(selected) = filter else {
        return Ok(Vec::new());
    };

    let existing: BTreeSet<String> = get_if_addrs()?
        .into_iter()
        .map(|iface| iface.name)
        .collect();
    let missing = selected
        .iter()
        .filter(|iface| !existing.contains(*iface))
        .cloned()
        .collect();
    Ok(missing)
}

fn selected_ips(filter: &InterfaceFilter, snapshot: &[InterfaceSnapshot]) -> Vec<IpAddr> {
    if matches!(filter, InterfaceFilter::All) {
        return Vec::new();
    }

    let mut ips: Vec<IpAddr> = snapshot.iter().map(|item| item.ip).collect();
    ips.sort();
    ips.dedup();
    ips
}

fn fqdn_name(name: &str) -> String {
    if name.ends_with(".local") {
        name.to_owned()
    } else {
        format!("{name}.local")
    }
}

fn start_responder(
    name: &str,
    filter: &InterfaceFilter,
    snapshot: &[InterfaceSnapshot],
) -> io::Result<Responder> {
    let allowed_ips = selected_ips(filter, snapshot);
    let mut display_ips: Vec<IpAddr> = snapshot.iter().map(|item| item.ip).collect();
    display_ips.sort();
    display_ips.dedup();

    info!(
        "starting mdns responder: hostname={}, interfaces={}, visible_ips={:?}",
        fqdn_name(name),
        filter.as_log_value(),
        display_ips
    );
    debug!("responder allowed_ips={allowed_ips:?}");

    let (responder, task) =
        Responder::with_default_handle_and_ip_list_and_hostname(allowed_ips, name.to_owned())?;
    tokio::spawn(task);
    Ok(responder)
}

fn init_logger() {
    let mut builder = env_logger::Builder::new();
    builder
        .filter_level(log::LevelFilter::Info)
        .filter_module(env!("CARGO_PKG_NAME"), log::LevelFilter::Debug)
        .format_timestamp_secs()
        .format_target(false)
        .init();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger();

    let options = match parse_args() {
        Ok(value) => value,
        Err(err) => {
            eprintln!("argument error: {err}");
            let program = env::args().next().unwrap_or_else(|| "mdnsd".to_owned());
            print_usage(&program);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, err).into());
        }
    };

    let filter = InterfaceFilter::from_values(&options.interfaces);
    info!(
        "config loaded: name={}, interfaces={}",
        options.name,
        filter.as_log_value()
    );

    let missing = collect_missing_interfaces(&filter)?;
    if !missing.is_empty() {
        warn!("requested interfaces not found: {}", missing.join(","));
    }

    let mut snapshot = collect_snapshot(&filter)?;
    if snapshot.is_empty() {
        warn!("no matching non-loopback interfaces at startup");
    }
    debug!("initial interface snapshot={snapshot:?}");

    let mut responder = Some(start_responder(&options.name, &filter, &snapshot)?);

    let mut ticker = tokio::time::interval(Duration::from_secs(DEFAULT_POLL_SECONDS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("received Ctrl+C, shutting down");
                break;
            }
            _ = ticker.tick() => {
                let current_snapshot = match collect_snapshot(&filter) {
                    Ok(value) => value,
                    Err(err) => {
                        warn!("failed to refresh interface list: {err}");
                        continue;
                    }
                };

                if current_snapshot != snapshot {
                    info!("network interface change detected, restarting mdns responder");
                    debug!("old_snapshot={snapshot:?}");
                    debug!("new_snapshot={current_snapshot:?}");
                    snapshot = current_snapshot;

                    if let Some(old) = responder.take() {
                        drop(old);
                    }

                    match start_responder(&options.name, &filter, &snapshot) {
                        Ok(new_responder) => {
                            responder = Some(new_responder);
                            info!("mdns responder restarted");
                        }
                        Err(err) => {
                            error!("failed to restart mdns responder: {err}");
                        }
                    }
                }
            }
        }
    }

    if let Some(active) = responder.take() {
        drop(active);
    }

    Ok(())
}
