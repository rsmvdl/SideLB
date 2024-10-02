use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tokio::net::{TcpStream, UdpSocket};
use crate::modules::utils::log;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadBalancerMode {
    RoundRobin,
    LeastConnections,
}

impl std::str::FromStr for LoadBalancerMode {
    type Err = String;

    fn from_str(input: &str) -> Result<LoadBalancerMode, Self::Err> {
        match input.to_lowercase().as_str() {
            "round-robin" => Ok(LoadBalancerMode::RoundRobin),
            "least-connections" => Ok(LoadBalancerMode::LeastConnections),
            _ => Err(format!("Invalid load balancer mode: {}", input)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Protocol {
    TCP,
    UDP,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Backend {
    pub addr: SocketAddr,
    pub protocol: Protocol,
}

pub struct LoadBalancer {
    pub backends: Mutex<HashMap<String, Vec<Backend>>>,
    pub active_backends: Mutex<HashMap<String, Vec<Backend>>>,
    pub current: Mutex<HashMap<String, usize>>,
    pub mode: LoadBalancerMode,
    pub connection_counts: Mutex<HashMap<String, usize>>,
}

impl LoadBalancer {
    pub fn new(mode: LoadBalancerMode) -> Self {
        LoadBalancer {
            backends: Mutex::new(HashMap::new()),
            active_backends: Mutex::new(HashMap::new()),
            current: Mutex::new(HashMap::new()),
            mode,
            connection_counts: Mutex::new(HashMap::new()),
        }
    }

    pub async fn add_backends(&self, new_backend_groups: HashMap<String, Vec<(SocketAddr, Option<Protocol>)>>) {
        let mut all_configured_backends = self.backends.lock().await;
        let mut active_backends_map = self.active_backends.lock().await;
        let mut connection_counts_map = self.connection_counts.lock().await;
        let mut current_indices_map = self.current.lock().await;

        for (hostname_label, addrs_with_proto_opt) in new_backend_groups {
            log(format!("[LB Static Add] Processing backend group: {}", hostname_label));
            let mut resolved_backend_list: Vec<Backend> = Vec::new();
            for (addr, protocol_opt) in addrs_with_proto_opt {
                let determined_protocol = match protocol_opt {
                    Some(p) => p,
                    None => detect_protocol(addr).await.unwrap_or(Protocol::TCP),
                };
                resolved_backend_list.push(Backend {
                    addr,
                    protocol: determined_protocol,
                });
            }

            if resolved_backend_list.is_empty() {
                log(format!("[LB Static Add] No backends provided for group {}. Skipping.", hostname_label));
                all_configured_backends.remove(&hostname_label);
                active_backends_map.remove(&hostname_label);
                connection_counts_map.remove(&hostname_label);
            } else {
                log(format!("[LB Static Add] Adding/Replacing group {} with {} backends.", hostname_label, resolved_backend_list.len()));
                all_configured_backends.insert(hostname_label.clone(), resolved_backend_list.clone());
                active_backends_map.insert(hostname_label.clone(), resolved_backend_list);
                connection_counts_map.entry(hostname_label.clone()).or_insert(0);
                current_indices_map.entry(hostname_label).or_insert(0);
            }
        }
    }

    pub async fn update_dynamic_backends(
        &self,
        domain_label: &str,
        resolved_addrs_with_proto_opt: Vec<(SocketAddr, Option<Protocol>)>,
    ) {
        log(format!("[LB Dynamic Update] Updating backends for dynamic group: {}", domain_label));

        let mut new_backend_list_for_domain: Vec<Backend> = Vec::new();
        for (addr, protocol_opt) in resolved_addrs_with_proto_opt {
            let determined_protocol = match protocol_opt {
                Some(p) => p,
                None => detect_protocol(addr).await.unwrap_or(Protocol::TCP),
            };
            new_backend_list_for_domain.push(Backend {
                addr,
                protocol: determined_protocol,
            });
        }

        let mut all_configured_backends = self.backends.lock().await;
        if new_backend_list_for_domain.is_empty() {
            log(format!("[LB Dynamic Update] No backends resolved for {}. Removing group from configured backends.", domain_label));
            all_configured_backends.remove(domain_label);
        } else {
            log(format!("[LB Dynamic Update] Updating configured backends for group {} with {} resolved entries.", domain_label, new_backend_list_for_domain.len()));
            all_configured_backends.insert(domain_label.to_string(), new_backend_list_for_domain.clone());
        }
        drop(all_configured_backends);

        let mut active_backends_map = self.active_backends.lock().await;
        if new_backend_list_for_domain.is_empty() {
            if active_backends_map.remove(domain_label).is_some() {
                log(format!("[LB Dynamic Update] Removed active backend group for {} as it has no resolved (configured) backends.", domain_label));
            }
        } else {
            if let Some(active_list_for_this_domain) = active_backends_map.get_mut(domain_label) {
                let configured_addrs_for_domain_set: HashSet<SocketAddr> =
                    new_backend_list_for_domain.iter().map(|b| b.addr).collect();

                let original_active_count = active_list_for_this_domain.len();
                active_list_for_this_domain.retain(|backend| configured_addrs_for_domain_set.contains(&backend.addr));
                let pruned_count = original_active_count - active_list_for_this_domain.len();
                if pruned_count > 0 {
                    log(format!("[LB Dynamic Update] Pruned {} stale entries from active list for group {}.", pruned_count, domain_label));
                }

                if active_list_for_this_domain.is_empty() {
                    log(format!("[LB Dynamic Update] Active list for group {} is empty after pruning. Removing from active map.", domain_label));
                    active_backends_map.remove(domain_label);
                }
            }
        }
        drop(active_backends_map);

        if new_backend_list_for_domain.is_empty() {
            let mut counts = self.connection_counts.lock().await;
            if counts.remove(domain_label).is_some() {
                log(format!("[LB Dynamic Update] Removed connection counts for group {}.", domain_label));
            }
        }
        log(format!("[LB Dynamic Update] Finished update for group {}.", domain_label));
    }

    pub async fn next_backend(&self) -> Option<Backend> {
        let active_backends_map = self.active_backends.lock().await;
        let all_active_backends: Vec<Backend> = active_backends_map.values().flatten().cloned().collect();

        if all_active_backends.is_empty() {
            return None;
        }

        match self.mode {
            LoadBalancerMode::RoundRobin => {
                let mut current_indices = self.current.lock().await;
                let global_idx_key = "global_round_robin".to_string();
                let idx = current_indices.entry(global_idx_key).or_insert(0);
                if all_active_backends.is_empty() { return None; }

                let backend_to_return = all_active_backends.get(*idx)?.clone();
                *idx = (*idx + 1) % all_active_backends.len();
                Some(backend_to_return)
            }
            LoadBalancerMode::LeastConnections => {
                let connection_counts_map = self.connection_counts.lock().await;
                let mut least_connected_backend: Option<Backend> = None;
                let mut min_connections = usize::MAX;

                for (group_label, backends_in_group) in active_backends_map.iter() {
                    let group_connection_count = connection_counts_map.get(group_label).cloned().unwrap_or(0);
                    if group_connection_count < min_connections && !backends_in_group.is_empty() {
                        min_connections = group_connection_count;
                        least_connected_backend = Some(backends_in_group[0]);
                    } else if least_connected_backend.is_none() && !backends_in_group.is_empty() {
                        min_connections = group_connection_count;
                        least_connected_backend = Some(backends_in_group[0]);
                    }
                }
                if least_connected_backend.is_none() && !all_active_backends.is_empty() {
                    return Some(all_active_backends[0]);
                }
                least_connected_backend
            }
        }
    }

    pub async fn increment_connection(&self, backend_addr: SocketAddr) {
        let mut connection_counts_map = self.connection_counts.lock().await;
        let all_configured_backends = self.backends.lock().await;
        for (group_label, backends_in_group) in all_configured_backends.iter() {
            if backends_in_group.iter().any(|b| b.addr == backend_addr) {
                *connection_counts_map.entry(group_label.clone()).or_insert(0) += 1;
                break;
            }
        }
    }

    pub async fn decrement_connection(&self, backend_addr: SocketAddr) {
        let mut connection_counts_map = self.connection_counts.lock().await;
        let all_configured_backends = self.backends.lock().await;
        for (group_label, backends_in_group) in all_configured_backends.iter() {
            if backends_in_group.iter().any(|b| b.addr == backend_addr) {
                if let Some(count) = connection_counts_map.get_mut(group_label) {
                    if *count > 0 {
                        *count -= 1;
                    }
                }
                break;
            }
        }
    }

    pub async fn perform_health_checks(&self) {
        loop {
            sleep(Duration::from_secs(10)).await;
            let configured_backends_snapshot = self.backends.lock().await.clone();

            for (hostname_label, configured_ips_in_group) in configured_backends_snapshot {
                for backend_to_check in configured_ips_in_group {
                    let is_healthy = match backend_to_check.protocol {
                        Protocol::TCP => TcpStream::connect(backend_to_check.addr).await.is_ok(),
                        Protocol::UDP => {
                            if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
                                socket.send_to(b"health", backend_to_check.addr).await.is_ok()
                            } else {
                                false
                            }
                        }
                    };

                    let mut active_backends_map = self.active_backends.lock().await;
                    let active_list_for_group = active_backends_map
                        .entry(hostname_label.clone())
                        .or_insert_with(Vec::new);

                    let currently_in_active_list = active_list_for_group.iter().any(|b| b.addr == backend_to_check.addr);

                    if is_healthy {
                        if !currently_in_active_list {
                            active_list_for_group.push(backend_to_check);
                            log(format!("[Health Check] Backend {} ({:?}) is now Healthy and added to active list for group {}.", backend_to_check.addr, backend_to_check.protocol, hostname_label));
                        }
                    } else {
                        if currently_in_active_list {
                            active_list_for_group.retain(|b| b.addr != backend_to_check.addr);
                            log(format!("[Health Check] Backend {} ({:?}) is now Unhealthy and removed from active list for group {}.", backend_to_check.addr, backend_to_check.protocol, hostname_label));
                            if active_list_for_group.is_empty() {
                                active_backends_map.remove(&hostname_label);
                                log(format!("[Health Check] Active backend group {} is now empty and removed.", hostname_label));
                            }
                        }
                    }
                }
            }
        }
    }
}

pub async fn detect_protocol(addr: SocketAddr) -> Option<Protocol> {
    if tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(addr)).await.is_ok() {
        return Some(Protocol::TCP);
    }
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
        if tokio::time::timeout(Duration::from_secs(1), socket.send_to(b"probe", addr)).await.is_ok() {
            // return Some(Protocol::UDP); // Intentionally commented out as per original code
        }
    }
    None
}