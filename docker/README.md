# Docker-Based Distributed Deployment

This directory contains everything needed to simulate a **multi-machine PersistHotStuff deployment** using Docker containers, where each container acts as a separate host.

## Architecture

```
┌─────────────────────────────────────────────────┐
│              hotstuff-net (172.28.0.0/16)        │
│                                                   │
│  ┌───────────┐  ┌───────────┐  ┌───────────┐     │
│  │ replica-0  │  │ replica-1  │  │ replica-2  │    │
│  │ 172.28.1.1 │  │ 172.28.1.2 │  │ 172.28.1.3 │   │
│  │ Volume: R0 │  │ Volume: R1 │  │ Volume: R2 │   │
│  └─────┬──────┘  └─────┬──────┘  └─────┬──────┘   │
│        │               │               │           │
│        └───────────────┼───────────────┘           │
│                        │                           │
│                  ┌─────┴──────┐                    │
│                  │ replica-3  │                    │
│                  │ 172.28.1.4 │                    │
│                  │ Volume: R3 │                    │
│                  └────────────┘                    │
└─────────────────────────────────────────────────┘
```

Each replica container:
- Has its own **persistent volume** (WAL + snapshots survive restarts)
- Runs on a **dedicated IP** within a Docker bridge network
- Has **NET_ADMIN** capability for network fault injection

## Quick Start

### 1. Build the image

Run from the **project root** (not from `docker/`):

```bash
docker compose -f docker/docker-compose.yml build
```

### 2. Launch all 4 replicas

```bash
docker compose -f docker/docker-compose.yml up -d
```

### 3. Run evaluations inside the cluster

```bash
# Crash-recovery stress test
docker compose -f docker/docker-compose.yml run --rm orchestrator ./eval_crash_recovery

# Commit latency benchmark
docker compose -f docker/docker-compose.yml run --rm orchestrator ./eval_commit_latency

# Membership cycling
docker compose -f docker/docker-compose.yml run --rm orchestrator ./eval_membership_cycling
```

### 4. Simulate network faults

```bash
# Inject 50ms latency ± 10ms jitter + 1% packet loss
./docker/inject_network_faults.sh add

# View current rules
./docker/inject_network_faults.sh status

# Remove all shaping
./docker/inject_network_faults.sh remove
```

### 5. Crash / restart individual replicas

```bash
# Kill replica-1 (simulates machine crash)
docker compose -f docker/docker-compose.yml stop replica-1

# Restart it (triggers WAL + snapshot recovery)
docker compose -f docker/docker-compose.yml start replica-1
```

### 6. View logs

```bash
docker compose -f docker/docker-compose.yml logs -f replica-0
```

### 7. Tear down

```bash
docker compose -f docker/docker-compose.yml down -v   # -v removes volumes too
```

## Network Fault Injection

The `inject_network_faults.sh` script uses **Linux Traffic Control (tc/netem)** to simulate realistic WAN conditions:

| Parameter    | Default | Description |
|-------------|---------|-------------|
| `DELAY`     | 50ms    | Base one-way latency |
| `JITTER`    | 10ms    | Variation around the delay |
| `CORRELATION`| 25%    | Temporal correlation of jitter |
| `LOSS`      | 1%      | Packet loss probability |
| `DUPLICATE` | 0.1%    | Packet duplication probability |

Edit the variables at the top of the script to customize.

### Custom per-replica shaping

```bash
# High latency on replica-2 only (simulates geo-distant node)
docker exec hotstuff-replica-2 \
  tc qdisc add dev eth0 root netem delay 200ms 30ms loss 5%
```

## How This Simulates Separate Machines

| Real deployment concern | Docker simulation |
|------------------------|-------------------|
| Separate hosts         | Separate containers with isolated filesystems |
| Network communication  | Docker bridge network (172.28.x.x) |
| WAN latency/jitter     | `tc netem` inside each container |
| Packet loss            | `tc netem loss` parameter |
| Machine crash          | `docker compose stop <replica>` |
| Disk persistence       | Named Docker volumes per replica |
| Resource isolation     | Container CPU/memory limits (add to compose) |

## Adding Resource Constraints

```yaml
# In docker-compose.yml, add under each replica:
deploy:
  resources:
    limits:
      cpus: "1.0"
      memory: 512M
    reservations:
      cpus: "0.5"
      memory: 256M
```

## Scaling to More Replicas

To test with n=7 (f=2), add replica-4 through replica-6 in `docker-compose.yml` following the same pattern, update the `PEERS` environment variable on each, and add entries to `inject_network_faults.sh`.
