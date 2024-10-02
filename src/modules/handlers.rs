use tokio::net::{TcpStream, UdpSocket};
use tokio::io::split;
use std::sync::Arc;
use crate::modules::load_balancer::{LoadBalancer, Protocol};
use crate::modules::utils::log;

pub async fn handle_tcp(inbound: TcpStream, lb: Arc<LoadBalancer>) {
    let _client_addr = inbound.peer_addr().expect("Failed to get client address");
    let backend = {
        lb.next_backend().await
    };

    if let Some(backend) = backend {
        log(format!("Forwarding TCP connection to backend: {} (Protocol: {:?})", backend.addr, backend.protocol));
        lb.increment_connection(backend).await; // Increment connection count

        match backend.protocol {
            Protocol::TCP => {
                match TcpStream::connect(backend.addr).await {
                    Ok(outbound) => {
                        let (mut ri, mut wi) = split(inbound);
                        let (mut ro, mut wo) = split(outbound);

                        let client_to_server = tokio::spawn(async move {
                            if let Err(e) = tokio::io::copy(&mut ri, &mut wo).await {
                                eprintln!("Error forwarding from client to server: {:?}", e);
                            }
                        });

                        let server_to_client = tokio::spawn(async move {
                            if let Err(e) = tokio::io::copy(&mut ro, &mut wi).await {
                                eprintln!("Error forwarding from server to client: {:?}", e);
                            }
                        });

                        if let Err(e) = tokio::try_join!(client_to_server, server_to_client) {
                            eprintln!("Error joining copy tasks: {:?}", e);
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to backend: {}. Error: {:?}", backend.addr, e);
                    }
                }
            }
            Protocol::UDP => {
                eprintln!("Received a TCP connection, but backend expects UDP for backend: {}", backend.addr);
            }
        }

        lb.decrement_connection(backend).await; // Decrement connection count
    } else {
        eprintln!("No available backends to handle TCP request.");
    }
}

pub async fn handle_udp(socket: Arc<UdpSocket>, lb: Arc<LoadBalancer>) {
    let mut buf = vec![0; 1024];

    loop {
        if let Ok((len, addr)) = socket.recv_from(&mut buf).await {
            let backend = {
                lb.next_backend().await
            };

            if let Some(backend) = backend {
                log(format!("Forwarding UDP packet to backend: {} (Protocol: {:?})", backend.addr, backend.protocol));
                lb.increment_connection(backend).await; // Increment connection count

                match backend.protocol {
                    Protocol::UDP => {
                        if let Ok(backend_socket) = UdpSocket::bind("0.0.0.0:0").await {
                            if let Err(e) = backend_socket.send_to(&buf[..len], backend.addr).await {
                                eprintln!("Failed to send UDP packet to backend {}: {:?}", backend.addr, e);
                            }
                            let mut response_buf = vec![0; 1024];
                            if let Ok((resp_len, _)) = backend_socket.recv_from(&mut response_buf).await {
                                if let Err(e) = socket.send_to(&response_buf[..resp_len], addr).await {
                                    eprintln!("Failed to send UDP response to {}: {:?}", addr, e);
                                }
                            }
                        } else {
                            eprintln!("Failed to bind temporary UDP socket");
                        }
                    }
                    Protocol::TCP => {
                        eprintln!("Received a UDP packet, but backend expects TCP for backend: {}", backend.addr);
                    }
                }

                lb.decrement_connection(backend).await; // Decrement connection count
            } else {
                eprintln!("No available backends to handle UDP request.");
            }
        } else {
            eprintln!("Failed to receive UDP packet");
        }
    }
}
