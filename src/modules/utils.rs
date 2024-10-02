use chrono::Local;
use std::borrow::Cow;
use std::net::SocketAddr;
use std::collections::HashMap;
use crate::modules::load_balancer::{LoadBalancerMode, Protocol, LoadBalancer};
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Duration;

pub fn log(message: String) {
    let now = Local::now();
    println!("[{}] {}", now.format("%Y-%m-%d %H:%M:%S"), message);
}

pub fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    let indent = "  ";
    let key_column_width = 40;

    println!();
    println!("===============================");
    println!("    SideLB - Version {}", version);
    println!("===============================");
    println!();

    println!("Usage:");
    println!(
        "{}{}",
        indent,
        "sidelb <bind_addr:bind_port> [backends=ip1:port1,ip2:port2,...] [mode=<load_balancer_mode>] [proto=<tcp|udp>] [ring_domain=<ring_domain:port>]"
    );
    println!();

    println!("Arguments:");
    println!(
        "{}{:<width$}{}",
        indent,
        "<bind_addr:bind_port>",
        "Address to bind the load balancer (e.g., 127.0.0.1:5432)",
        width = key_column_width
    );
    println!();

    println!(
        "{}{:<width$}{}",
        indent,
        "[backends=ip1:port1,...]",
        "Comma-separated list of backend addresses (e.g., backends=127.0.0.1:8081,10.0.0.1:8082)",
        width = key_column_width
    );
    println!();

    println!(
        "{}{:<width$}{}",
        indent,
        "[mode=<load_balancer_mode>]",
        "Load balancer mode.",
        width = key_column_width
    );
    println!("{}{:<width$}{}", indent, "", "  Default: round-robin.", width = key_column_width);
    println!("{}{:<width$}{}", indent, "", "  Available: 'round-robin', 'least-connections'.", width = key_column_width);
    println!();


    println!(
        "{}{:<width$}{}",
        indent,
        "[proto=<tcp|udp>]",
        "Protocol for the load balancer.",
        width = key_column_width
    );
    println!("{}{:<width$}{}", indent, "", "  Default: TCP.", width = key_column_width);
    println!("{}{:<width$}{}", indent, "", "  Choose between TCP and UDP.", width = key_column_width);
    println!();

    println!(
        "{}{:<width$}{}",
        indent,
        "[ring_domain=<ring_domain:port>]",
        "A hostname that resolves to multiple backend IP addresses.",
        width = key_column_width
    );
    println!();

    println!("Options:");
    println!(
        "{}{:<width$}{}",
        indent,
        "-h, --help",
        "Display this help message and exit.",
        width = key_column_width
    );
    println!();

    println!(
        "{}{:<width$}{}",
        indent,
        "--health-check-uds",
        "Perform a health check via the default UDS path (/run/sidelb.sock)",
        width = key_column_width
    );
    println!("{}{:<width$}{}", indent, "", "  and exit with status 0 (healthy) or 1 (unhealthy).", width = key_column_width);
    println!();

    println!("Examples:");
    println!("{}{}", indent, "# Basic TCP load balancing to two static backends (defaults to round-robin, TCP):");
    println!("{}{}{}", indent, indent, "sidelb 0.0.0.0:8080 backends=192.168.1.10:80,192.168.1.11:80");
    println!();

    println!("{}{}", indent, "# UDP load balancing using least-connections mode:");
    println!("{}{}{}", indent, indent, "sidelb 0.0.0.0:5353 backends=10.0.0.1:53,10.0.0.2:53 mode=least-connections proto=udp");
    println!();

    println!("{}{}", indent, "# TCP load balancing using a ring_domain for dynamic backends on localhost:");
    println!("{}{}{}", indent, indent, "sidelb 127.0.0.1:3000 ring_domain=my-app-backends.local:8000 proto=tcp");
    println!();

    println!("{}{}", indent, "# Combine static backends (via backends=) with a ring_domain (all using TCP):");
    println!("{}{}{}", indent, indent, "sidelb 0.0.0.0:9000 backends=10.1.0.5:9001 ring_domain=dynamic-nodes.example.com:9002 proto=tcp");
    println!();
}

pub fn parse_arguments(args: &[String]) -> (SocketAddr, HashMap<String, Vec<SocketAddr>>, Option<String>, LoadBalancerMode, Protocol) {
    if args.is_empty() {
        print_help();
        panic!("Insufficient arguments: At least bind_addr is required.");
    }

    let bind_addr: SocketAddr = args[0].parse().expect("Invalid bind address");
    let mut backend_groups: HashMap<String, Vec<SocketAddr>> = HashMap::new();
    let mut ring_domain: Option<String> = None;
    let mut mode = LoadBalancerMode::RoundRobin;
    let mut proto = Protocol::TCP;

    for arg in &args[1..] {
        if arg.starts_with("backends=") {
            let addrs_str = &arg["backends=".len()..];
            if addrs_str.is_empty() {
                eprintln!("Warning: 'backends=' argument provided but no addresses listed.");
                continue;
            }
            for addr_s in addrs_str.split(',') {
                let trimmed_addr_s = addr_s.trim();
                if trimmed_addr_s.is_empty() {
                    continue;
                }
                match trimmed_addr_s.parse::<SocketAddr>() {
                    Ok(addr) => {
                        let host = addr.ip().to_string();
                        backend_groups.entry(host).or_insert_with(Vec::new).push(addr);
                    }
                    Err(e) => {
                        eprintln!("Warning: Could not parse backend address '{}' from 'backends=' list: {}. Skipping.", trimmed_addr_s, e);
                    }
                }
            }
        } else if arg.starts_with("ring_domain=") {
            ring_domain = Some(arg["ring_domain=".len()..].to_string());
        } else if arg.starts_with("mode=") {
            mode = arg["mode=".len()..].parse().expect("Invalid load balancer mode");
        } else if arg.starts_with("proto=") {
            proto = match arg["proto=".len()..].to_lowercase().as_str() {
                "udp" => Protocol::UDP,
                "tcp" => Protocol::TCP,
                _ => panic!("Invalid protocol: must be 'tcp' or 'udp'"),
            };
        } else if arg == "-h" || arg == "--help" {
            continue;
        } else {
            eprintln!("Warning: Unrecognized argument or option: '{}'. It will be ignored.", arg);
        }
    }
    (bind_addr, backend_groups, ring_domain, mode, proto)
}

pub async fn run_uds_status_server(uds_path: &str, lb: Arc<LoadBalancer>) {
    match tokio::fs::remove_file(uds_path).await {
        Ok(_) => log(format!("[UDS Status] Removed existing socket file: {}", uds_path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {},
        Err(e) => {
            eprintln!("[UDS Status] ERROR: Error removing existing socket file {}: {:?}. Status server might not start correctly.", uds_path, e);
        }
    }

    match UnixListener::bind(uds_path) {
        Ok(listener) => {
            log(format!("[UDS Status] Server listening on {}", uds_path));
            loop {
                match listener.accept().await {
                    Ok((mut stream, _client_addr)) => {
                        let lb_clone = lb.clone();
                        tokio::spawn(async move {
                            let mut buffer = [0; 1];
                            match stream.read(&mut buffer).await {
                                Ok(0) => {
                                    log("[UDS Status] Client connected and closed (EOF). Processing health check.".to_string());
                                }
                                Ok(_) => {
                                    log(format!("[UDS Status] Client sent data (byte: {}). Processing health check.", buffer[0]));
                                }
                                Err(e) => {
                                    eprintln!("[UDS Status] Error reading from UDS stream: {:?}. Assuming query anyway.", e);
                                }
                            }

                            let active_backends_map = lb_clone.active_backends.lock().await;
                            let is_healthy = active_backends_map.values().any(|backends| !backends.is_empty());
                            let response_str = if is_healthy { "HEALTHY\n" } else { "UNHEALTHY\n" };

                            if let Err(e) = stream.write_all(response_str.as_bytes()).await {
                                eprintln!("[UDS Status] Error writing response: {:?}", e);
                            }
                            if let Err(e) = stream.flush().await {
                                eprintln!("[UDS Status] Error flushing UDS stream: {:?}", e);
                            }
                            if let Err(e) = stream.shutdown().await {
                                eprintln!("[UDS Status] Error shutting down UDS stream: {:?}", e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("[UDS Status] Error accepting UDS connection: {:?}", e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
            eprintln!("[UDS Status] CRITICAL: Could not bind UDS status server to {}: {:?}", uds_path, e);
            eprintln!("Health checks via UDS will not be available.");
            eprintln!("This likely means the Docker HEALTHCHECK will fail if it targets this UDS.");
            eprintln!("Check permissions and if the path is already in use by a non-socket file.");
            eprintln!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
        }
    }
}

pub async fn perform_uds_health_check(uds_path: &str) {
    log(format!("[UDS Check] Performing health check against UDS: {}", uds_path));

    let connect_timeout = Duration::from_secs(5);
    let read_timeout = Duration::from_secs(5);

    match tokio::time::timeout(connect_timeout, UnixStream::connect(uds_path)).await {
        Ok(Ok(mut stream)) => {
            if let Err(e) = stream.write_all(b"Q").await {
                eprintln!("[UDS Check] Failed to send query byte to {}: {:?}", uds_path, e);
                std::process::exit(1);
            }
            if let Err(e) = stream.flush().await {
                eprintln!("[UDS Check] Failed to flush UDS stream for {}: {:?}", uds_path, e);
                std::process::exit(1);
            }
            if let Err(e) = stream.shutdown().await {
                eprintln!("[UDS Check] Failed to shutdown write half of UDS stream {}: {:?}", uds_path, e);
            }

            let mut response_buffer = Vec::with_capacity(128);

            match tokio::time::timeout(read_timeout, stream.read_to_end(&mut response_buffer)).await {
                Ok(Ok(bytes_read)) => {
                    if bytes_read == 0 {
                        eprintln!("[UDS Check] Failed: Server at {} closed connection without response.", uds_path);
                        std::process::exit(1);
                    }
                    let cow_response: Cow<str> = String::from_utf8_lossy(&response_buffer[..bytes_read]);
                    let response_str: &str = cow_response.trim();

                    if response_str == "HEALTHY" {
                        log(format!("[UDS Check] Successful: Received '{}'", response_str));
                        println!("Healthy");
                        std::process::exit(0);
                    } else {
                        eprintln!("[UDS Check] Failed: Server at {} responded with '{}'", uds_path, response_str);
                        std::process::exit(1);
                    }
                }
                Ok(Err(e)) => {
                    eprintln!("[UDS Check] Failed to read response from {}: {:?}", uds_path, e);
                    std::process::exit(1);
                }
                Err(_) => {
                    eprintln!("[UDS Check] Timeout reading response from {}", uds_path);
                    std::process::exit(1);
                }
            }
        }
        Ok(Err(e)) => {
            eprintln!("[UDS Check] Failed to connect to {}: {:?}", uds_path, e);
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("[UDS Check] Timeout connecting to {}", uds_path);
            std::process::exit(1);
        }
    }
}