use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tokio::net::{TcpStream, UdpSocket};
use crate::modules::utils::log;

#[derive(Debug, Clone, Copy)]
pub enum LoadBalancerMode {
    RoundRobin,
    LeastConnections,
}

impl std::str::FromStr for LoadBalancerMode {
    type Err = ();

    fn from_str(input: &str) -> Result<LoadBalancerMode, Self::Err> {
        match input.to_lowercase().as_str() {
            "round-robin" => Ok(LoadBalancerMode::RoundRobin),
            "least-connections" => Ok(LoadBalancerMode::LeastConnections),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Protocol {
    TCP,
    UDP,
}

#[derive(Debug, Clone, Copy)]
pub struct Backend {
    pub addr: SocketAddr,
    pub protocol: Protocol,
}

pub struct LoadBalancer {
    pub backends: Mutex<HashMap<String, Vec<Backend>>>,  // Group backends by hostname
    pub active_backends: Mutex<HashMap<String, Vec<Backend>>>,  // Active backends by hostname
    pub current: Mutex<HashMap<String, usize>>,  // Current index for each hostname group
    pub mode: LoadBalancerMode,
    pub connection_counts: Mutex<HashMap<String, usize>>,  // Track connections by hostname group
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

    pub async fn add_backends(&self, new_backends: HashMap<String, Vec<(SocketAddr, Option<Protocol>)>>) {
        let mut backends = self.backends.lock().await;
        let mut active_backends = self.active_backends.lock().await;
        let mut connection_counts = self.connection_counts.lock().await;
        let mut current = self.current.lock().await;

        for (hostname, ips) in new_backends {
            let mut backend_list: Vec<Backend> = Vec::new();

            for (addr, protocol) in ips {
                let determined_protocol = if let Some(p) = protocol {
                    p // Use the explicitly provided protocol if available
                } else {
                    // Dynamically determine protocol (TCP or UDP)
                    detect_protocol(addr).await.unwrap_or_else(|| Protocol::TCP)
                };

                backend_list.push(Backend {
                    addr,
                    protocol: determined_protocol,
                });
            }

            // Insert into the backends and active_backends HashMaps
            backends.insert(hostname.clone(), backend_list.clone());
            active_backends.insert(hostname.clone(), backend_list.clone());

            // Initialize connection counts and round-robin index
            connection_counts.entry(hostname.clone()).or_insert(0);
            current.entry(hostname).or_insert(0); // Initialize round-robin index
        }

        log(format!("Added backends: {:?}", backends));
    }

    pub async fn next_backend(&self) -> Option<Backend> {
        let active_backends = self.active_backends.lock().await;

        // Flatten all IP addresses from all hostnames into a single list
        let all_backends: Vec<Backend> = active_backends.values().flatten().cloned().collect();

        if all_backends.is_empty() {
            log("No active backends available.".to_string());
            return None;
        }

        match self.mode {
            LoadBalancerMode::RoundRobin => {
                let mut current = self.current.lock().await;

                // Ensure there is an entry for round-robin index
                let idx = current.entry("global".to_string()).or_insert(0);
                let backend = all_backends.get(*idx)?.clone();  // Clone the Backend struct

                // Advance to the next IP in the list, wrapping around
                *idx = (*idx + 1) % all_backends.len();
                Some(backend)  // Return the cloned backend
            },
            LoadBalancerMode::LeastConnections => {
                let connection_counts = self.connection_counts.lock().await;

                // Find the backend with the least connections
                let mut least_connected = None;
                let mut least_connections = usize::MAX;

                for (hostname, backends) in active_backends.iter() {
                    for backend in backends {
                        if let Some(&count) = connection_counts.get(hostname) {
                            if count < least_connections {
                                least_connections = count;
                                least_connected = Some(*backend);
                            }
                        }
                    }
                }

                least_connected
            },
        }
    }

    pub async fn increment_connection(&self, backend: Backend) {
        let mut connection_counts = self.connection_counts.lock().await;
        for (hostname, ips) in self.backends.lock().await.iter() {
            if ips.iter().any(|b| b.addr == backend.addr) {
                *connection_counts.entry(hostname.clone()).or_insert(0) += 1;
                break;
            }
        }
    }

    pub async fn decrement_connection(&self, backend: Backend) {
        let mut connection_counts = self.connection_counts.lock().await;
        for (hostname, ips) in self.backends.lock().await.iter() {
            if ips.iter().any(|b| b.addr == backend.addr) {
                if let Some(count) = connection_counts.get_mut(hostname) {
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
            sleep(Duration::from_secs(10)).await;  // Perform health checks every 10 seconds
            let backends = self.backends.lock().await.clone();

            for (hostname, ips) in backends {
                for backend in ips {
                    match backend.protocol {
                        Protocol::TCP => {
                            match TcpStream::connect(backend.addr).await {
                                Ok(_) => {
                                    // Backend is reachable, ensure it is in the active list
                                    let mut active_backends = self.active_backends.lock().await;
                                    let active_ips = active_backends.entry(hostname.clone()).or_insert_with(Vec::new);
                                    if !active_ips.iter().any(|b| b.addr == backend.addr) {
                                        active_ips.push(backend);
                                        log(format!("Backend {} is back online and marked as healthy.", backend.addr));
                                    }
                                }
                                Err(_) => {
                                    // Backend is unreachable, remove it from the active list
                                    let mut active_backends = self.active_backends.lock().await;
                                    if let Some(active_ips) = active_backends.get_mut(&hostname) {
                                        if let Some(pos) = active_ips.iter().position(|b| b.addr == backend.addr) {
                                            active_ips.remove(pos);
                                            log(format!("Backend {} is offline and marked as unhealthy.", backend.addr));
                                        }
                                    }
                                }
                            }
                        }
                        Protocol::UDP => {
                            // Perform UDP health check by attempting to bind a UDP socket
                            match UdpSocket::bind("0.0.0.0:0").await {
                                Ok(udp_socket) => {
                                    let health_check_msg = b"health-check";
                                    if udp_socket.send_to(health_check_msg, backend.addr).await.is_ok() {
                                        // Backend is reachable, ensure it is in the active list
                                        let mut active_backends = self.active_backends.lock().await;
                                        let active_ips = active_backends.entry(hostname.clone()).or_insert_with(Vec::new);
                                        if !active_ips.iter().any(|b| b.addr == backend.addr) {
                                            active_ips.push(backend);
                                            log(format!("UDP Backend {} is back online and marked as healthy.", backend.addr));
                                        }
                                    } else {
                                        log(format!("UDP Backend {} is not responding.", backend.addr));
                                    }
                                }
                                Err(_) => {
                                    log(format!("Failed to bind UDP socket for health check on backend {}", backend.addr));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// Helper function to detect the protocol dynamically by attempting to connect to the backend
pub async fn detect_protocol(addr: SocketAddr) -> Option<Protocol> {
    // Test TCP connection first
    if TcpStream::connect(addr).await.is_ok() {
        return Some(Protocol::TCP);
    }

    // If TCP fails, test UDP connection by attempting to send a small message
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
        let test_msg = b"protocol_test";
        if socket.send_to(test_msg, addr).await.is_ok() {
            return Some(Protocol::UDP);
        }
    }

    // If both tests fail, return None
    None
}
