use chrono::Local;
use std::net::SocketAddr;
use std::collections::HashMap;
use crate::modules::load_balancer::{LoadBalancerMode, Protocol};

pub fn log(message: String) {
    let now = Local::now();
    println!("[{}] {}", now.format("%Y-%m-%d %H:%M:%S"), message);
}

pub fn print_help() {
    let version = env!("CARGO_PKG_VERSION");
    println!();
    println!("===============================");
    println!("    SideLB - Version {}", version);
    println!("===============================");
    println!();
    println!("Usage:");
    println!("  sidelb <bind_addr:bind_port> [backend_addr1:port] [mode=<load_balancer_mode>] [proto=<tcp|udp>] [ring_domain=<ring_domain:port>]");
    println!();
    println!("Arguments:");
    println!("  <bind_addr:bind_port>                 Address to bind the load balancer (e.g., 127.0.0.1:5432)");
    println!("  [backend_addr1:port ...]              List of backend addresses (e.g., 127.0.0.1:8081)");
    println!("  [mode=<load_balancer_mode>]           Load balancer mode (e.g., round-robin, least-connections). Default is round-robin.");
    println!("  [proto=<tcp|udp>]                     Protocol to use for the load balancer choose between TCP and UDP. Default is TCP if not set.");
    println!("  [ring_domain=<ring_domain:port>]      A hostname that resolves to multiple backend IP addresses.");
    println!();
    println!("Options:");
    println!("  -h, --help                            Display this help message and exit");
    println!();
}

pub fn parse_arguments(args: &[String]) -> (SocketAddr, HashMap<String, Vec<SocketAddr>>, Option<String>, LoadBalancerMode, Protocol) {
    if args.len() < 1 {
        panic!("Insufficient arguments");
    }

    let bind_addr: SocketAddr = args[0].parse().expect("Invalid bind address");
    let mut backend_groups: HashMap<String, Vec<SocketAddr>> = HashMap::new();
    let mut ring_domain: Option<String> = None;
    let mut mode = LoadBalancerMode::RoundRobin;
    let mut proto = Protocol::TCP; // Default to TCP

    for arg in &args[1..] {
        if arg.starts_with("ring_domain=") {
            ring_domain = Some(arg["ring_domain=".len()..].to_string());
        } else if arg.starts_with("mode=") {
            mode = arg["mode=".len()..].parse().expect("Invalid load balancer mode");
        } else if arg.starts_with("proto=") {
            proto = match arg["proto=".len()..].to_lowercase().as_str() {
                "udp" => Protocol::UDP,
                "tcp" => Protocol::TCP,
                _ => panic!("Invalid protocol"),
            };
        } else {
            let addr: SocketAddr = arg.parse().expect("Invalid backend address");
            let host = addr.ip().to_string();
            backend_groups.entry(host).or_insert_with(Vec::new).push(addr);
        }
    }

    (bind_addr, backend_groups, ring_domain, mode, proto)
}
