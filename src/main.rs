mod modules;

use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;

use modules::dns::resolve_ring_domain;
use modules::handlers::{handle_tcp, handle_udp};
use modules::load_balancer::{LoadBalancer, Protocol};
use modules::utils::{log, print_help, parse_arguments, perform_uds_health_check, run_uds_status_server};

use tokio::net::{TcpListener, UdpSocket};
use tokio::time::Duration;

const DEFAULT_UDS_PATH: &str = "/run/sidelb.sock";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() >= 2 && args[1] == "--health-check-uds" {
        perform_uds_health_check(DEFAULT_UDS_PATH).await;
        return Ok(());
    }

    if args.len() < 2 || args.contains(&String::from("--help")) || args.contains(&String::from("-h")) {
        print_help();
        return Ok(());
    }
    
    let (bind_addr, static_backend_groups_from_args, ring_domain_option, mode, global_protocol) = parse_arguments(&args[1..]);

    log(format!(
        "Starting load balancer on address: {} with protocol: {:?} and mode: {:?}",
        bind_addr, global_protocol, mode
    ));

    let lb = Arc::new(LoadBalancer::new(mode));

    // Add statically configured backends first
    if !static_backend_groups_from_args.is_empty() {
        let mut static_backends_for_lb = HashMap::new();
        for (group_label, addrs) in static_backend_groups_from_args {
            let addrs_with_proto: Vec<(SocketAddr, Option<Protocol>)> = addrs
                .into_iter()
                .map(|addr| (addr, Some(global_protocol)))
                .collect();
            static_backends_for_lb.insert(group_label, addrs_with_proto);
        }
        lb.add_backends(static_backends_for_lb).await;
    }

    // Handle ring_domain: initial resolution and setting up periodic re-resolution
    if let Some(ref ring_domain_str_as_ref) = ring_domain_option {
        let ring_domain_str_owned_for_initial = ring_domain_str_as_ref.clone();
        let ring_domain_str_owned_for_task = ring_domain_str_as_ref.clone();

        log(format!("[Initial Ring] Resolving ring address: {}", ring_domain_str_owned_for_initial));

        let resolved_backends_initial = resolve_ring_domain(&ring_domain_str_owned_for_initial, global_protocol).await;

        if resolved_backends_initial.is_empty() {
            log(format!("[Initial Ring] Warning: Failed to resolve ring domain '{}' or no backends found initially.", ring_domain_str_owned_for_initial));
        }

        // Use update_dynamic_backends for the initial population.
        // The domain_label for this group will be the ring_domain_str itself.
        lb.update_dynamic_backends(&ring_domain_str_owned_for_initial, resolved_backends_initial).await;

        log(format!("[Periodic Ring] Setting up periodic re-resolution for {} every 60 seconds", ring_domain_str_owned_for_task));

        let lb_clone_for_ring_update = lb.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await; // Wait for the next 60-second interval
                log(format!("[Periodic Ring] Re-resolving ring address: {}", ring_domain_str_owned_for_task));

                let resolved_backends_periodic = resolve_ring_domain(&ring_domain_str_owned_for_task, global_protocol).await;

                if resolved_backends_periodic.is_empty() {
                    log(format!("[Periodic Ring] Warning: Re-resolution of ring domain '{}' yielded no backends.", ring_domain_str_owned_for_task));
                }

                lb_clone_for_ring_update.update_dynamic_backends(&ring_domain_str_owned_for_task, resolved_backends_periodic).await;
            }
        });
    }

    // Check if any backends are configured after initial setup.
    // `ring_domain_option` is still valid here due to the `ref` pattern used above.
    if lb.backends.lock().await.is_empty() {
        if ring_domain_option.is_none() { // If no ring domain was configured AND static backends were also empty.
            eprintln!("Error: No static backends configured AND no ring_domain specified. Load balancer has no backend destinations.");
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "No backends configured. Load balancer cannot start.",
            ).into());
        } else {
            // A ring domain was specified but might not have resolved any backends *yet*, or resolved to none.
            // The periodic task will keep trying.
            log("Warning: No backends currently in the load balancer after initial setup, but a ring_domain is configured and will be polled.".to_string());
        }
    }

    let lb_clone_for_backend_hc = lb.clone();
    tokio::spawn(async move {
        lb_clone_for_backend_hc.perform_health_checks().await;
    });

    let lb_clone_for_uds_status = lb.clone();
    tokio::spawn(async move {
        run_uds_status_server(DEFAULT_UDS_PATH, lb_clone_for_uds_status).await;
    });
    log(format!("UDS Status server configured at default path: {}", DEFAULT_UDS_PATH));

    match global_protocol {
        Protocol::TCP => {
            let tcp_listener = TcpListener::bind(bind_addr).await?;
            let tcp_lb_main_clone = lb.clone();
            log(format!("TCP listener started on: {}", bind_addr));
            tokio::spawn(async move {
                loop {
                    match tcp_listener.accept().await {
                        Ok((inbound, _)) => {
                            let tcp_lb_conn_clone = tcp_lb_main_clone.clone();
                            tokio::spawn(async move {
                                handle_tcp(inbound, tcp_lb_conn_clone).await;
                            });
                        }
                        Err(e) => eprintln!("Failed to accept TCP connection: {:?}", e),
                    }
                }
            });
        }
        Protocol::UDP => {
            let udp_socket = Arc::new(UdpSocket::bind(bind_addr).await?);
            let udp_lb_main_clone = lb.clone();
            log(format!("UDP listener started on: {}", bind_addr));
            tokio::spawn(async move {
                handle_udp(udp_socket, udp_lb_main_clone).await;
            });
        }
    }

    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
