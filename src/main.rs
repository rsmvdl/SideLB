mod modules;

use std::collections::HashMap;
use modules::load_balancer::{LoadBalancer, Protocol};
use modules::handlers::{handle_tcp, handle_udp};
use modules::utils::{log, print_help, parse_arguments};
use modules::dns::resolve_ring_domain;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args.contains(&String::from("--help")) || args.contains(&String::from("-h")) {
        print_help();
        return Ok(());
    }

    // Parse arguments and determine protocol
    let (bind_addr, backend_addrs, ring_domain, mode, proto) = parse_arguments(&args[1..]);

    log(format!(
        "Starting load balancer on address: {} with protocol: {:?} and mode: {:?}",
        bind_addr, proto, mode
    ));

    let lb = Arc::new(LoadBalancer::new(mode));

    // Add backend addresses provided directly
    let mut backends_with_protocol = HashMap::new();
    for (hostname, ips) in backend_addrs {
        let backend_list: Vec<(SocketAddr, Option<Protocol>)> = ips
            .into_iter()
            .map(|addr| (addr, Some(proto))) // Use the provided protocol
            .collect();
        backends_with_protocol.insert(hostname, backend_list);
    }
    lb.add_backends(backends_with_protocol).await;

    // If a ring domain is provided, resolve and add its backends
    if let Some(ring_domain) = ring_domain {
        log(format!("Resolving ring address: {}", ring_domain));
        let resolved_backends = resolve_ring_domain(&ring_domain, proto).await;

        if resolved_backends.is_empty() {
            eprintln!("Failed to resolve ring domain or no backends found.");
            return Ok(()); // Exit the program if no backends are found
        }

        let mut resolved_groups: HashMap<String, Vec<(SocketAddr, Option<Protocol>)>> = HashMap::new();
        for (addr, detected_protocol) in resolved_backends {
            let host = addr.ip().to_string();
            resolved_groups
                .entry(host)
                .or_insert_with(Vec::new)
                .push((addr, Some(detected_protocol.unwrap_or(proto))));
        }

        lb.add_backends(resolved_groups).await;
    }

    // Start the health check task
    let lb_clone = lb.clone();
    tokio::spawn(async move {
        lb_clone.perform_health_checks().await;
    });

    // Start the appropriate listener (TCP or UDP) based on the protocol selected
    match proto {
        Protocol::TCP => {
            let tcp_listener = TcpListener::bind(bind_addr).await?;
            let tcp_lb = lb.clone();
            log(format!("TCP listener started on: {}", bind_addr));
            tokio::spawn(async move {
                loop {
                    match tcp_listener.accept().await {
                        Ok((inbound, _)) => {
                            let tcp_lb = tcp_lb.clone();
                            tokio::spawn(async move {
                                handle_tcp(inbound, tcp_lb).await;
                            });
                        }
                        Err(e) => eprintln!("Failed to accept TCP connection: {:?}", e),
                    }
                }
            });
        }
        Protocol::UDP => {
            let udp_socket = Arc::new(UdpSocket::bind(bind_addr).await?);
            let udp_lb = lb.clone();
            log(format!("UDP listener started on: {}", bind_addr));
            tokio::spawn(async move {
                handle_udp(udp_socket, udp_lb).await;
            });
        }
    }

    // Keep the main task alive
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
