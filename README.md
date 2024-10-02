# SideLB – Sidecar Load Balancer, a lightweight service load balancer that runs beside your actual Application

SideLB manages traffic between your service (a server backend, …) and other services (databases, caching clusters, …). It provides robust and efficient load balancing for backends that require seamless integration with decentralized services, no matter the protocol.

SideLB supports infrastructures built for decentralized applications. E.g., you can run it in the same container as your application, this is actually the idea.

SideLB is written in Rust and primarily based on the tokio library.

## Key features

- **Perform DNS and reverse DNS (rDNS) resolution** to discover both IPv4 and IPv6 members of a domain. For example, in a decentralized CockroachDB or YugabyteDB setup, a domain like db.example.com may resolve to multiple IP addresses (e.g., 10 IPv4 and 10 IPv6 addresses). SideLB leverages DNS/rDNS resolution to intelligently distribute traffic across all available servers.
- **Group multiple IP addresses belonging to the same server and treats them as a single entity**. This prevents server with multiple public IPs from being overburdened. (Note that DNS/rDNS must be setup correctly!)
- **No protocol overhead**. SideLB is purpose-built for TCP/UDP traffic and does not handle HTTP or other service-specific protocols directly. This makes it particularly suited for routing database queries and other low-level service communications.
- **Continuously monitor the availability of each server**, ensuring traffic is only routed to healthy servers. It supports two key load balancing algorithms: 
    1) round-robin: Evenly distributes traffic across all available servers, and 
    2) least-connections: Routes traffic to the server with the fewest active connections.

## Setup structure example

```
                                        +----------------------+
                                        |       Request        |
                                        +----------------------+
                                                   |
                                                   v
                                        +----------------------+
                                        | Backend Application  |
                                        +----------------------+
                                                   |
                                                   v
                                        +----------------------+
                                        |         SideLB       |   
                                        |    127.0.0.1:5432    |
+-----------------------------------------------------------------------------------------------------+ -> db.example.com
        |                    |                     |                    |                    |
        v                    v                     v                    v                    v
+-----------------+  +-----------------+  +-----------------+  +-----------------+  +-----------------+
|  PostgreSQL 1   |  |  PostgreSQL 2   |  |  PostgreSQL 3   |  |  PostgreSQL 4   |  |  PostgreSQL 5   |
| 203.0.113.1:5432|  | 203.0.113.2:5432|  | 203.0.113.3:5432|  | 203.0.113.4:5432|  | 203.0.113.5:5432|
| 2001:db8::1:5432|  | 2001:db8::2:5432|  | 2001:db8::3:5432|  | 2001:db8::4:5432|  | 2001:db8::5:5432|
+-----------------+  +-----------------+  +-----------------+  +-----------------+  +-----------------+
```

# Usage

Your backend connects to SideLB, which then forwards the traffic to the appropriate backend services.
You can specify either static IP addresses for backend services or a ring domain (ring_domain) for dynamic DNS resolution.

Example Command:

```bash
sidelb 127.0.0.1:5432 ring_domain=db.example.com:5432 mode=round-robin
```

In this example:

- `127.0.0.1:5432` is the address where SideLB listens for incoming connections from your backend.
- `ring_domain=db.example.com` allows SideLB to dynamically resolve backend IPs using DNS, e.g. your CockroachDB cluster.
- `mode=round-robin` ensures that traffic is evenly distributed across all resolved backend service members.
- `proto=tcp/udp` Set the desired protocol to use, you can select between TCP and UDP

Command with static IP addresses:

```bash
sidelb 127.0.0.1:5432 100.100.100.103:5432 100.100.100.104:5432 mode=least-connections
```

Here, SideLB forwards traffic from `127.0.0.1:5432` to `100.100.100.103:5432` and `100.100.100.104:5432`.

Additionally, you can also manually select the protocol you want to load balance (TCP/UDP), just simply do:

```bash
sidelb 127.0.0.1:5432 100.100.100.103:5432 100.100.100.104:5432 mode=least-connections proto=tcp
```
or using the ring_domain respectively
```bash
sidelb 127.0.0.1:5432 ring_domain=db.example.com:5432 mode=round-robin proto=tcp
```
and so on


This will 

## Known Limitations

- **Load balancing is only relative with SideLB, as most likely many containers or servers consuming a service like a Database and SideLB instances don't communicate with each other at all ...
- **SSL/TLS Name Validation:** When using SSL/TLS between the backend and the target servers, name validation may need to be disabled or explicitly add `127.0.0.1` to your certs. Since the backend communicates with SideLB over `127.0.0.1`, which then forwards the request to the target server, the original domain name might not match the SSL certificate. This can lead to SSL name validation errors unless explicitly turned off.

  For example, when you're using the Django framework you would set the following for your Database settings:
  ```
    'sslmode': 'verify-ca',  # Changed from 'verify-full' to 'verify-ca' to skip hostname verification
  ```

## How to build?
Compiling SideLB is fairly easy, you simply run the included trigger.sh script locally, this will spin-up an Ubuntu 24.04
container compiling the software for you (Docker required), the final executable binary is than available at the /build folder
as the compiled binary is simply getting copied from the Docker Container to your local filesystem.
```bash
./trigger.sh
```

## Implementation example inside a container

Many developers that build containers using supervisord to manage processes spawned inside there container(s).
The same way you can do it with SideLB, simply include the binary at e.g. /usr/bin of you container, make it executable
and start an instance like so:

```
[program:SideLB]
user=my_user
autostart=true
autorestart=true
command=sidelb_flow
stdout_logfile=/dev/stdout
stdout_logfile_maxbytes=0
stdout_logfile_backups=0
redirect_stderr=true
killasgroup=true
stopasgroup=true
priority=100

[program:ExampleApp]
user=my_user
autostart=true
autorestart=true
command=app_exec_command
stdout_logfile=/dev/stdout
stdout_logfile_maxbytes=0
stdout_logfile_backups=0
redirect_stderr=true
killasgroup=true
stopasgroup=true
priority=200
```

the sidelb_flow script is just an example here, you can also implement this otherwise, but this is how I did it in combination
with env variables:

```
#!/usr/bin/env bash

function sidelb_exec {
echo "Starting SideLB"

# Use environment variables to form the sidelb command
sidelb 127.0.0.1:${DATABASE_PORT} ring_domain="${DATABASE_ENDPOINT}:${DATABASE_PORT}" mode=${SIDELB_MODE}
}

sidelb_exec
```
With this setup, your application connects to the database through 127.0.0.1:${DATABASE_PORT}, e.g. port 5432 for postgres.
SideLB then resolves the ring_domain, which points to the actual database service consisting of multiple IPs, 
and forwards the request to one of these resolved IP addresses using the selected load balancing algorithm.


## Who Should Use SideLB?

- **Developers working with microservices or decentralized services like Redis, CockroachDB or YugabyteDB, where traffic needs to be efficiently routed across multiple service nodes.
- **Teams deploying applications where an embedded load balancer is required for efficient traffic management like communicating with decentralized services.
- **Environments where external load balancers are unnecessary or add unwanted complexity, as SideLB provides a lightweight, embedded alternative.

## Requirements

- **A Domain name under your control
- **A service you want to reach using SideLB, e.g. CockRoachDB or similar that exists on the Public Web or private infrastructure

## Final tough's and notices
I developed SideLB as part of a larger project that heavily relays on decentralization, especially for the Database communication. 
This way I wasn't in need anymore to manage and/or rent load balancer resources on platforms like AWS, Digitalocean, Google etc.
as the application itself is now able to load balance connections by itself with a certain service endpoint.

If you set up a decentralized service like a CockRoachDB or similar, consider to split them up into Geo-Zones on DNS side, like:

- us.database.com (100.100.100.103, 100.100.100.104, 100.100.100.105)
  - -> node01.us.my-service.com - 100.100.100.103
  - -> node02.us.my-service.com - 100.100.100.104
  - -> node03.us.my-service.com - 100.100.100.105
- de.database.com (101.100.100.103, 101.100.100.104, 101.100.100.105)
  - -> node01.de.my-service.com - 101.100.100.103
  - -> node02.de.my-service.com - 101.100.100.104
  - -> node03.de.my-service.com - 101.100.100.105
- fr.database.com (102.100.100.103, 102.100.100.104, 102.100.100.105)
  - -> node01.fr.my-service.com - 102.100.100.103
  - -> node02.fr.my-service.com - 102.100.100.104
  - -> node03.fr.my-service.com - 102.100.100.105

Ensure that each node’s IP points to its corresponding geographic subdomain like us.database.com ...
This way you can make sure that your application always communicates with a group of servers that are geographically near to each other.
