use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use trust_dns_resolver::TokioAsyncResolver;
use trust_dns_resolver::config::*;
use std::collections::HashMap;
use crate::modules::utils::log;
use crate::modules::load_balancer::Protocol;

pub async fn resolve_ring_domain(ring_domain: &str, protocol: Protocol) -> Vec<(SocketAddr, Option<Protocol>)> {
    let mut result = Vec::new();
    let mut ip_map: HashMap<String, Vec<(SocketAddr, String)>> = HashMap::new();

    // Split the ring_domain into hostname and port if port is specified
    let (hostname, port) = match ring_domain.split_once(':') {
        Some((host, port)) => {
            match port.parse::<u16>() {
                Ok(p) => (host, p),
                Err(_) => {
                    log(format!("Invalid port provided for {}: please specify a valid port", ring_domain));
                    return result; // Return early if the port is invalid
                }
            }
        },
        None => {
            log(format!("No port specified for {}: a port is required!", ring_domain));
            return result; // Return early if no port is specified
        }
    };

    // Resolve hostname to a list of SocketAddr using to_socket_addrs()
    match (hostname, port).to_socket_addrs() {
        Ok(iter) => {
            for socket_addr in iter {
                let rdns_name = resolve_rdns_name(socket_addr.ip()).await.unwrap_or_else(|| "<unknown>".to_string());

                // Use the provided protocol, either UDP or TCP
                result.push((socket_addr, Some(protocol)));
                ip_map.entry(rdns_name.clone()).or_insert_with(Vec::new).push((socket_addr, rdns_name));
            }
            for (rdns_name, addresses) in ip_map {
                let ip_list: Vec<String> = addresses.iter().map(|(addr, _)| addr.to_string()).collect();
                log(format!(
                    "Resolved {} to {} ({})",
                    hostname,
                    ip_list.join(", "),
                    rdns_name
                ));
            }
        }
        Err(e) => eprintln!("Failed to resolve ring address {}: {:?}", ring_domain, e),
    }

    result
}

pub async fn resolve_rdns_name(ip: IpAddr) -> Option<String> {
    // Create an async DNS resolver
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

    // Perform reverse DNS lookup
    match resolver.reverse_lookup(ip).await {
        Ok(names) => names.iter().next().map(|name| name.to_string()),
        Err(_) => None, // Return None if reverse lookup fails
    }
}
