#!/usr/bin/env python3

import argparse
import logging
import os
import re
import signal
from pathlib import Path
from threading import Event
from typing import Iterable

from prometheus_client import disable_created_metrics, start_http_server
from prometheus_client.core import CollectorRegistry, CounterMetricFamily, Metric
from prometheus_client.registry import Collector

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("bees-prometheus-exporter")

disable_created_metrics()  # type: ignore

stop_event = Event()
signal.signal(signal.SIGTERM, lambda signum, frame: stop_event.set())
signal.signal(signal.SIGINT, lambda signum, frame: stop_event.set())


class BeesCollector(Collector):
    stats_dir: Path
    pattern = re.compile(r"(\w+)=(\d+)")

    def __init__(self, stats_dir: Path):
        self.stats_dir = stats_dir
        fd = -1
        try:
            fd = os.open(self.stats_dir, os.O_RDONLY | os.O_DIRECTORY)
        finally:
            if fd != -1:
                os.close(fd)

    def collect(self) -> Iterable[Metric]:
        values: dict[str, tuple[dict[str, int], float]] = {}
        for file in self.stats_dir.glob("*.status"):
            values[file.stem] = self._collect_stats_from_file(file)

        counters: dict[str, CounterMetricFamily] = {}
        for uuid, (metrics, timestamp) in values.items():
            for metric_name, value in metrics.items():
                counter = counters.get(metric_name)
                if counter is None:
                    logger.debug(f"Creating counter for metric {metric_name}")
                    counter = CounterMetricFamily(
                        f"bees_{metric_name.lower()}_total",
                        f"Bees metric {metric_name}",
                        labels=["uuid"],
                    )
                logger.debug(f"Adding metric {metric_name} with value {value} for uuid {uuid}")
                counter.add_metric([uuid], value, timestamp=timestamp)
                counters[metric_name] = counter
        yield from counters.values()

    def _collect_stats_from_file(self, stats_file: Path) -> tuple[dict[str, int], float]:
        if not stats_file.exists():
            logger.warning(f"Status file {stats_file} does not exist.")
            return {}, 0.0

        with stats_file.open() as f:
            timestamp = os.fstat(f.fileno()).st_mtime
            logger.debug(f"Reading stats from {stats_file}")
            lines = f.readlines()

        ret: dict[str, int] = {}
        total_head_found = False
        for line in lines:
            if line.startswith("TOTAL:"):
                total_head_found = True
                continue
            if not total_head_found:
                logger.error("TOTAL line not found in stats file.")
                return {}, 0.0
            if line.startswith("RATES:"):
                break
            for metric in line.split(" "):
                metric = metric.strip()
                match = self.pattern.match(metric)
                if not match:
                    logger.warning(f"Unrecognized element in metrics line: {metric}")
                    continue

                metric_name, value = match.groups()
                ret[metric_name] = int(value)
        if len(ret) == 0:
            logger.error(f"No metrics found in stats file {stats_file}")
        return ret, timestamp


def main() -> None:
    """Main entry point for the bees prometheus exporter."""
    parser = argparse.ArgumentParser(description="Expose bees stats as Prometheus metrics.")

    parser.add_argument(
        "--bees-work-dir",
        type=Path,
        default=Path("/run/bees/"),
        help="Path to bees work directory.",
    )
    parser.add_argument("--log-level", type=str, default="INFO", help="Logging level.")

    parser.add_argument("--port", type=int, default=8080, help="Port to expose metrics on.")
    parser.add_argument("--listen-address", type=str, default="::0", help="Bind address for the HTTP server.")

    args = parser.parse_args()

    logger.setLevel(args.log_level)  # type: ignore

    registry = CollectorRegistry()
    collector = BeesCollector(args.bees_work_dir)  # type: ignore
    registry.register(collector)
    start_http_server(
        args.port,  # type: ignore
        addr=args.listen_address,  # type: ignore
        registry=registry,
    )
    logger.info(f"Serving metrics on port {args.port}")  # type: ignore

    stop_event.wait()
    logger.info("Shutting down.")


if __name__ == "__main__":
    main()
