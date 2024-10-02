use tokio::net::{TcpStream, UdpSocket};
use tokio::io::{split};
use std::sync::Arc;
use tokio::time::Duration;
use crate::modules::load_balancer::{LoadBalancer, Protocol};
use crate::modules::utils::log;

pub async fn handle_tcp(inbound: TcpStream, lb: Arc<LoadBalancer>) {
    let client_peer_addr_result = inbound.peer_addr(); // Get peer address once

    let backend_option = lb.next_backend().await;

    if let Some(selected_backend) = backend_option {
        // Define client_addr_str and backend_addr_for_logs in the outer scope
        let client_addr_str = match client_peer_addr_result {
            Ok(addr) => addr.to_string(),
            Err(_) => "unknown_client".to_string(),
        };
        let backend_addr_for_logs = selected_backend.addr; // SocketAddr is Copy

        log(format!("[TCP] Client {} attempting connection. Forwarding to backend: {} (Protocol: {:?})", client_addr_str, backend_addr_for_logs, selected_backend.protocol));

        lb.increment_connection(selected_backend.addr).await;

        match selected_backend.protocol {
            Protocol::TCP => {
                match TcpStream::connect(selected_backend.addr).await {
                    Ok(outbound) => {
                        log(format!("[TCP] Established connection from {} to backend {}", client_addr_str, backend_addr_for_logs));
                        let (mut ri, mut wi) = split(inbound);
                        let (mut ro, mut wo) = split(outbound);

                        // Clone variables for the first spawned task
                        let client_addr_c2s = client_addr_str.clone();
                        let backend_addr_c2s = backend_addr_for_logs;
                        let client_to_server = tokio::spawn(async move {
                            match tokio::io::copy(&mut ri, &mut wo).await {
                                Ok(bytes) => {
                                    log(format!("[TCP] {} -> {}: Forwarded {} bytes from client to server.", client_addr_c2s, backend_addr_c2s, bytes));
                                    Ok(bytes)
                                }
                                Err(e) => {
                                    log(format!("[TCP] Error {} -> {}: Forwarding client to server: {:?}", client_addr_c2s, backend_addr_c2s, e));
                                    Err(e)
                                }
                            }
                        });

                        // Clone variables for the second spawned task
                        let client_addr_s2c = client_addr_str.clone();
                        let backend_addr_s2c = backend_addr_for_logs;
                        let server_to_client = tokio::spawn(async move {
                            match tokio::io::copy(&mut ro, &mut wi).await {
                                Ok(bytes) => {
                                    log(format!("[TCP] {} <- {}: Forwarded {} bytes from server to client.", client_addr_s2c, backend_addr_s2c, bytes));
                                    Ok(bytes)
                                }
                                Err(e) => {
                                    log(format!("[TCP] Error {} <- {}: Forwarding server to client: {:?}", client_addr_s2c, backend_addr_s2c, e));
                                    Err(e)
                                }
                            }
                        });

                        // Original client_addr_str and backend_addr_for_logs are still valid here
                        match tokio::try_join!(client_to_server, server_to_client) {
                            Ok((res_c2s_join, res_s2c_join)) => { // Results from JoinHandle (already unwrapped JoinError)
                                // res_c2s_join and res_s2c_join are Result<u64, io::Error>
                                let c2s_ok = res_c2s_join.is_ok();
                                let s2c_ok = res_s2c_join.is_ok();

                                if c2s_ok && s2c_ok {
                                    log(format!("[TCP] Connection {} <-> {} completed successfully.", client_addr_str, backend_addr_for_logs));
                                } else {
                                    log(format!("[TCP] Connection {} <-> {} completed with I/O errors. c2s_ok: {}, s2c_ok: {}", client_addr_str, backend_addr_for_logs, c2s_ok, s2c_ok));
                                    // Errors already logged within tasks
                                }
                            }
                            Err(join_err) => { // One of the tasks panicked
                                log(format!("[TCP] Task join error for connection {} <-> {}: {:?}", client_addr_str, backend_addr_for_logs, join_err));
                            }
                        }
                    }
                    Err(e) => {
                        log(format!("[TCP] Failed to connect to backend {}: {}. Client: {}", selected_backend.addr, e, client_addr_str));
                    }
                }
            }
            Protocol::UDP => {
                log(format!("[TCP] Protocol mismatch for client {}: Received TCP, but backend {} expects UDP. Dropping connection.", client_addr_str, selected_backend.addr));
            }
        }
        lb.decrement_connection(selected_backend.addr).await;
    } else {
        log(format!("[TCP] No available backends for client {}. Dropping connection.", client_peer_addr_result.map_or_else(|_| "unknown_client".to_string(), |a| a.to_string())));
    }
}

pub async fn handle_udp(socket: Arc<UdpSocket>, lb: Arc<LoadBalancer>) {
    let mut buf = vec![0; 2048];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, src_addr)) => {
                let backend_option = lb.next_backend().await;

                if let Some(selected_backend) = backend_option {
                    log(format!("[UDP] Client {} sent packet. Forwarding to backend: {} (Protocol: {:?})", src_addr, selected_backend.addr, selected_backend.protocol));

                    lb.increment_connection(selected_backend.addr).await;

                    match selected_backend.protocol {
                        Protocol::UDP => {
                            match UdpSocket::bind("0.0.0.0:0").await {
                                Ok(outbound_socket) => {
                                    if let Err(e) = outbound_socket.send_to(&buf[..len], selected_backend.addr).await {
                                        log(format!("[UDP] Failed to send packet from {} to backend {}: {:?}", src_addr, selected_backend.addr, e));
                                    } else {
                                        let mut response_buf = vec![0; 2048];
                                        match tokio::time::timeout(Duration::from_secs(5), outbound_socket.recv_from(&mut response_buf)).await {
                                            Ok(Ok((resp_len, backend_resp_addr))) => {
                                                log(format!("[UDP] Received response from {} (for backend {}) for client {}. Forwarding.", backend_resp_addr, selected_backend.addr, src_addr));
                                                if let Err(e) = socket.send_to(&response_buf[..resp_len], src_addr).await {
                                                    log(format!("[UDP] Failed to send response from backend {} to client {}: {:?}", selected_backend.addr, src_addr, e));
                                                }
                                            }
                                            Ok(Err(e)) => {
                                                log(format!("[UDP] Error receiving response from backend {} for client {}: {:?}", selected_backend.addr, src_addr, e));
                                            }
                                            Err(_) => {
                                                log(format!("[UDP] Timeout receiving response from backend {} for client {}. No response forwarded.", selected_backend.addr, src_addr));
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    log(format!("[UDP] Failed to bind temporary outbound UDP socket for client {}: {:?}", src_addr, e));
                                }
                            }
                        }
                        Protocol::TCP => {
                            log(format!("[UDP] Protocol mismatch for client {}: Received UDP, but backend {} expects TCP.", src_addr, selected_backend.addr));
                        }
                    }
                    lb.decrement_connection(selected_backend.addr).await;
                } else {
                    log(format!("[UDP] No available backends for client {}. Packet dropped.", src_addr));
                }
            }
            Err(e) => {
                log(format!("[UDP] Error receiving on main UDP socket: {:?}. Loop continues.", e));
            }
        }
    }
}