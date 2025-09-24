# Bees Prometheus Exporter

A Prometheus metrics exporter for the [Bees](https://github.com/zygo/bees) deduplication daemon.

## Installation

Install directly from crates.io:

```bash
cargo install bees-prometheus-exporter
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

- `-b, --bees-work-dir PATH`: Path to Bees work directory (default: `/run/bees`)
- `-p, --port PORT`: Port to bind the HTTP server to (default: `8080`)
- `-a, --address ADDRESS`: Address to bind the HTTP server to (default: `::0`)
- `-l, --log-level LEVEL`: Logging level - error, warn, info, debug, trace (default: `info`)

### Examples

Bind to localhost only:

```bash
bees-prometheus-exporter --address 127.0.0.1
```

Enable debug logging:

```bash
bees-prometheus-exporter --log-level debug
```

## Metrics

The exporter reads Bees status files (`<fs-uuid>.status`) from bees' stats directory, by default `/run/bees`.

The available metrics are described in https://github.com/Zygo/bees/blob/master/docs/event-counters.md

Additionally, the exporter reads the per-extent-size progress summary.

### Metric Format

All metrics follow the pattern `bees_{metric_name}_total` and include a `uuid` label identifying the filesystem.

The per-extent-size progress summary is formatted as `bees_progress_summary_{column_name}`, with the columns `datasz_bytes`, `point`, `gen_min` and `gen_max`. The extent size is provided as a label.  
When `point` is idle, `bees_progress_summary_point_idle` reports 1, else 0

## Configuration

Note that, by default, `/run/bees` is root-owned. The exporter requires read access to the directory.  
The exporter uses landlock to confine filesystem and network access. Landlock operates on fds, not paths, which means that deleting and recreating the bees directory will not show up in the exporter.

### Prometheus Configuration

Add the exporter to your Prometheus configuration:

```yaml
scrape_configs:
  - job_name: "bees"
    static_configs:
      - targets: ["localhost:8080"]
    scrape_interval: 1s # bees updates the stats file once per second
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

- Rust 1.85+ (for building from source)
- Bees deduplication daemon running and producing status files
