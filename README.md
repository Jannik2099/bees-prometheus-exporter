# Bees Prometheus Exporter

A Prometheus metrics exporter for the [Bees](https://github.com/zygo/bees) deduplication daemon.

## Installation

### From Source

Clone the repository and install:

```bash
git clone https://github.com/Jannik2099/bees-prometheus-exporter
cd bees-prometheus-exporter
pip install .
```

### Using uv

```bash
git clone https://github.com/Jannik2099/bees-prometheus-exporter
cd bees-prometheus-exporter
uv sync
```

## Usage

Basic usage with default settings:

```bash
bees-prometheus-exporter
```

The exporter will start on port 8080 and read Bees statistics from `/run/bees/`.

### Command Line Options

```bash
bees-prometheus-exporter [OPTIONS]
```

- `--bees-work-dir PATH`: Path to Bees work directory (default: `/run/bees/`)
- `--port PORT`: Port to expose metrics on (default: `8080`)
- `--listen-address ADDRESS`: Bind address for the HTTP server (default: `::0`)
- `--log-level LEVEL`: Logging level (default: `INFO`)

### Examples

Run on a custom port:

```bash
bees-prometheus-exporter --port 9090
```

Use a different Bees work directory:

```bash
bees-prometheus-exporter --bees-work-dir /var/lib/bees/
```

Bind to localhost only:

```bash
bees-prometheus-exporter --listen-address 127.0.0.1
```

Enable debug logging:

```bash
bees-prometheus-exporter --log-level DEBUG
```

## Metrics

The exporter reads Bees status files (`<fs-uuid>.status`) from bees' work directory, by default `/run/bees`.

The available metrics are described in https://github.com/Zygo/bees/blob/master/docs/event-counters.md

### Metric Format

All metrics follow the pattern `bees_{metric_name}_total` and include a `uuid` label identifying the filesystem.

## Configuration

Note that, by default, `/run/bees` is root-owned. The exporter requires read access to the directory.

### Prometheus Configuration

Add the exporter to your Prometheus configuration:

```yaml
scrape_configs:
  - job_name: "bees"
    static_configs:
      - targets: ["localhost:8080"]
    scrape_interval: 30s # bees updates the stats file once per second
```

## Monitoring Examples

### Grafana Dashboard Queries

Deduplication ratio:

```promql
rate(bees_bytes_deduped_total[5m]) / rate(bees_bytes_scanned_total[5m])
```

Deduplication rate in MiB/s:

```promql
rate(bees_bytes_deduped_total[5m]) / 1024 / 1024
```

Files processed per second:

```promql
rate(bees_files_scanned_total[5m])
```

## Requirements

- Python 3.9+
- `prometheus-client` library
- Bees deduplication daemon running and producing status files
