SideLB (Sidecar Load Balancer) is a lightweight, efficient **TCP/UDP service load balancer** and **Layer 4 reverse proxy**.
It's designed to distribute network traffic across your backend services, acting as a public-facing entry point or running alongside your application (as a "sidecar"). This makes it especially useful for decentralized services and containerized environments using Docker, Podman, or containerd.

SideLB is written in Rust and primarily based on the Tokio library.

## Key Features

-   **TCP/UDP Load Balancing:** Distributes traffic for both TCP and UDP based services.
-   **Layer 4 Reverse Proxy:** Forwards network connections/packets at the transport layer without inspecting application-layer data.
-   **Flexible Backend Discovery:**
    -   **Dynamic DNS (`ring_domain`):** Discovers backend instances by resolving a domain name (A/AAAA records).
    -   **Static IP List:** Allows specification of fixed backend IP addresses.
    -   **Unified Pool:** Treats dynamically discovered and statically listed backends as a single pool for load balancing.
-   **Load Balancing Algorithms:**
    -   `round-robin`: Evenly distributes connections.
    -   `least-connections`: Sends traffic to the server with the fewest active connections.
-   **Health Monitoring:** Continuously checks backend server availability and routes traffic only to healthy instances.
-   **IP Grouping:** Can group multiple ports on the same backend IP address for health checking and connection management.
-   **Container-Friendly:** Easily deployable with Docker, Podman, containerd, and configurable via command-line arguments or environment variables (when using the provided Docker entrypoint).
-   **Lightweight & Efficient:** Minimal resource footprint.

## Configuration via Environment Variables

When running SideLB using the provided Docker image and its entrypoint (`sidelb-daemon` command or default execution), you can configure it using the following environment variables. These variables are used by the entrypoint script to construct the necessary command-line arguments for the `sidelb` executable.

---

**`SIDELB_BIND_ADDR`**

* **Description:** Specifies the IP address and port on which SideLB will listen for incoming traffic. This is the primary listening interface for the load balancer.
* **Mandatory/Optional:** Mandatory.
* **Default Value:** None. The entrypoint script will exit with an error if this is not set.
* **Example:** `SIDELB_BIND_ADDR="0.0.0.0:8080"` (Listens on port 8080 on all available network interfaces)
* **Corresponding CLI Argument:** `<bind_addr:bind_port>` (the first positional argument)

---

**`SIDELB_BACKENDS`**

* **Description:** Provides a comma-separated list of static backend server addresses, each in `IP:port` format. SideLB will distribute traffic among these configured servers.
* **Mandatory/Optional:** Optional, but SideLB requires backend servers to operate. At least one of `SIDELB_BACKENDS` or `SIDELB_RING_DOMAIN` (or both) must be configured with valid backend information for SideLB to start and route traffic.
* **Default Value:** None (empty).
* **Example:** `SIDELB_BACKENDS="192.168.1.10:80,192.168.1.11:8000"`
* **Corresponding CLI Argument:** `backends=<value>` (e.g., `backends=192.168.1.10:80,192.168.1.11:8000`)

---

**`SIDELB_MODE`**

* **Description:** Sets the load balancing algorithm used to distribute traffic to backend servers.
* **Mandatory/Optional:** Optional.
* **Default Value:** `round-robin`
* **Available Values:**
    * `round-robin`: Distributes connections sequentially among the available healthy backends.
    * `least-connections`: Sends new connections to the healthy backend with the fewest active connections.
* **Example:** `SIDELB_MODE="least-connections"`
* **Corresponding CLI Argument:** `mode=<value>` (e.g., `mode=least-connections`)

---

**`SIDELB_PROTO`**

* **Description:** Defines the network protocol (TCP or UDP) for which SideLB will balance traffic. All backends (static and dynamic) will be assumed to operate over this protocol.
* **Mandatory/Optional:** Optional.
* **Default Value:** `tcp`
* **Available Values:** `tcp`, `udp`
* **Example:** `SIDELB_PROTO="udp"`
* **Corresponding CLI Argument:** `proto=<value>` (e.g., `proto=udp`)

---

**`SIDELB_RING_DOMAIN`**

* **Description:** Specifies a DNS domain name that SideLB will periodically resolve to discover backend IP addresses dynamically. SideLB will look for A/AAAA records for this domain. The port specified as part of this value is the port SideLB will attempt to connect to on the resolved IP addresses.
* **Mandatory/Optional:** Optional, but SideLB requires backend servers to operate. At least one of `SIDELB_BACKENDS` or `SIDELB_RING_DOMAIN` (or both) must be configured with valid backend information for SideLB to start and route traffic.
* **Default Value:** None.
* **Example:** `SIDELB_RING_DOMAIN="my-app-backends.example.com:8000"` (SideLB will resolve `my-app-backends.example.com` and connect to the resulting IPs on port `8000`)
* **Corresponding CLI Argument:** `ring_domain=<value>` (e.g., `ring_domain=my-app-backends.example.com:8000`)

---

## Known Limitations

-   **No Layer 7 Features:** SideLB does not inspect or manipulate application-layer data (e.g., HTTP headers, paths, cookies).
    It is not a replacement for Layer 7 proxies like Nginx or HAProxy if you need those features.

## Future Ideas / Roadmap

-   **Basic DDoS Protection Mechanisms:** Simple measures such as connection rate limiting per IP or basic SYN flood filtering at the TCP level.
