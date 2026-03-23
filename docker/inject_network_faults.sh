#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────────
#  inject_network_faults.sh
#
#  Uses Linux Traffic Control (tc/netem) inside Docker containers
#  to simulate realistic WAN conditions between PersistHotStuff replicas.
#
#  Usage:
#    ./inject_network_faults.sh add    # inject latency + loss
#    ./inject_network_faults.sh remove # remove all shaping rules
#    ./inject_network_faults.sh status # show current tc rules
#
#  Prerequisites: containers must have NET_ADMIN capability.
# ──────────────────────────────────────────────────────────────────
set -euo pipefail

REPLICAS=("hotstuff-replica-0" "hotstuff-replica-1" "hotstuff-replica-2" "hotstuff-replica-3")

# Simulated WAN parameters
DELAY="50ms"
JITTER="10ms"
CORRELATION="25%"
LOSS="1%"
DUPLICATE="0.1%"

add_faults() {
    echo "═══ Injecting network faults on ${#REPLICAS[@]} replicas ═══"
    echo "  Delay: ${DELAY} ± ${JITTER} (corr ${CORRELATION})"
    echo "  Loss:  ${LOSS}   Dup: ${DUPLICATE}"
    echo ""

    for container in "${REPLICAS[@]}"; do
        echo "  → ${container}"
        docker exec "${container}" \
            tc qdisc add dev eth0 root netem \
                delay "${DELAY}" "${JITTER}" "${CORRELATION}" \
                loss "${LOSS}" \
                duplicate "${DUPLICATE}" \
            2>/dev/null || \
        docker exec "${container}" \
            tc qdisc change dev eth0 root netem \
                delay "${DELAY}" "${JITTER}" "${CORRELATION}" \
                loss "${LOSS}" \
                duplicate "${DUPLICATE}"
    done
    echo ""
    echo "Done. Use '$0 status' to verify."
}

remove_faults() {
    echo "═══ Removing network faults ═══"
    for container in "${REPLICAS[@]}"; do
        echo "  → ${container}"
        docker exec "${container}" tc qdisc del dev eth0 root 2>/dev/null || true
    done
    echo "Done."
}

show_status() {
    echo "═══ Current tc rules ═══"
    for container in "${REPLICAS[@]}"; do
        echo "── ${container} ──"
        docker exec "${container}" tc qdisc show dev eth0 2>/dev/null || echo "  (no rules)"
        echo ""
    done
}

case "${1:-help}" in
    add)    add_faults ;;
    remove) remove_faults ;;
    status) show_status ;;
    *)
        echo "Usage: $0 {add|remove|status}"
        echo "  add    - inject latency, jitter, loss"
        echo "  remove - remove all shaping rules"
        echo "  status - show current tc rules"
        exit 1
        ;;
esac
