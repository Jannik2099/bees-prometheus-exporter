#!/usr/bin/env python3

import argparse
import logging
import os
import re
import signal
from collections.abc import Iterable, Iterator
from enum import Enum
from pathlib import Path
from threading import Event
from typing import Literal, NamedTuple, Optional

from prometheus_client import disable_created_metrics, start_http_server
from prometheus_client.core import (
    CollectorRegistry,
    CounterMetricFamily,
    GaugeMetricFamily,
    Metric,
)
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

    class ProgressRow(NamedTuple):
        extsz: Literal["max", "32M", "8M", "2M", "512K", "128K"]
        datasz: str
        point: int | Literal["idle"]
        gen_min: int
        gen_max: int

    def __init__(self, stats_dir: Path):
        self.stats_dir = stats_dir
        fd = -1
        try:
            fd = os.open(self.stats_dir, os.O_RDONLY | os.O_DIRECTORY)
        finally:
            if fd != -1:
                os.close(fd)

    def collect(self) -> Iterable[Metric]:
        values: dict[str, tuple[dict[str, int], list[BeesCollector.ProgressRow], float]] = {}
        for file in self.stats_dir.glob("*.status"):
            values[file.stem] = self._collect_stats_from_file(file)

        stats_counters: dict[str, CounterMetricFamily] = {}
        counter_datasz = GaugeMetricFamily(
            "bees_progress_summary_datasz_bytes",
            "Bees progress summary datasz in bytes",
            labels=["uuid", "extent_size"],
        )
        counter_point = GaugeMetricFamily(
            "bees_progress_summary_point",
            "Bees progress summary",
            labels=["uuid", "extent_size"],
        )
        counter_point_idle = GaugeMetricFamily(
            "bees_progress_summary_point_idle",
            "Bees progress summary idle",
            labels=["uuid", "extent_size"],
        )
        counter_gen_min = GaugeMetricFamily(
            "bees_progress_summary_gen_min",
            "Bees progress summary gen_min",
            labels=["uuid", "extent_size"],
        )
        counter_gen_max = GaugeMetricFamily(
            "bees_progress_summary_gen_max",
            "Bees progress summary gen_max",
            labels=["uuid", "extent_size"],
        )

        for uuid, (metrics, progress_rows, timestamp) in values.items():
            for metric_name, value in metrics.items():
                counter = stats_counters.get(metric_name)
                if counter is None:
                    logger.debug(f"Creating counter for metric {metric_name}")
                    counter = CounterMetricFamily(
                        f"bees_{metric_name.lower()}_total",
                        f"Bees metric {metric_name}",
                        labels=["uuid"],
                    )
                logger.debug(f"Adding metric {metric_name} with value {value} for uuid {uuid}")
                counter.add_metric([uuid], value, timestamp=timestamp)
                stats_counters[metric_name] = counter

            for progress_row in progress_rows:
                datasz_bytes = self._datasz_to_bytes(progress_row.datasz)
                if datasz_bytes is not None:
                    counter_datasz.add_metric(
                        [uuid, progress_row.extsz],
                        datasz_bytes,
                        timestamp=timestamp,
                    )
                else:
                    logger.error(f"Could not parse datasz for uuid {uuid}, row {progress_row}")
                if progress_row.point == "idle":
                    counter_point_idle.add_metric(
                        [uuid, progress_row.extsz],
                        1,
                        timestamp=timestamp,
                    )
                else:
                    counter_point_idle.add_metric(
                        [uuid, progress_row.extsz],
                        0,
                        timestamp=timestamp,
                    )
                    counter_point.add_metric(
                        [uuid, progress_row.extsz],
                        progress_row.point,
                        timestamp=timestamp,
                    )
                counter_gen_min.add_metric(
                    [uuid, progress_row.extsz],
                    progress_row.gen_min,
                    timestamp=timestamp,
                )
                counter_gen_max.add_metric(
                    [uuid, progress_row.extsz],
                    progress_row.gen_max,
                    timestamp=timestamp,
                )

        yield from stats_counters.values()
        yield counter_datasz
        yield counter_point
        yield counter_point_idle
        yield counter_gen_min
        yield counter_gen_max

    def _collect_stats_from_file(self, stats_file: Path) -> tuple[dict[str, int], list[ProgressRow], float]:
        try:
            with stats_file.open() as f:
                timestamp = os.fstat(f.fileno()).st_mtime
                logger.debug(f"Reading stats from {stats_file}")
                lines = f.readlines()
        except FileNotFoundError:
            logger.warning(f"Status file {stats_file} does not exist.")
            return {}, [], 0.0

        ret: dict[str, int] = {}
        progress_rows: list[BeesCollector.ProgressRow] = []

        class ParserState(Enum):
            NONE = 0
            TOTAL = 1
            RATES = 2
            PROGRESS = 3

        parser_state = ParserState.NONE
        line_iter = iter(lines)
        for line in line_iter:
            if line.startswith("TOTAL:"):
                parser_state = ParserState.TOTAL
                continue
            if line.startswith("RATES:"):
                parser_state = ParserState.RATES
                continue
            if line.startswith("PROGRESS:"):
                parser_state = ParserState.PROGRESS
                progress_rows = self._parse_progress_lines(line_iter)
                continue
            if parser_state == ParserState.RATES or parser_state == ParserState.NONE:
                continue
            if parser_state == ParserState.TOTAL:
                ret.update(self._parse_total_line(line))
        if len(ret) == 0:
            logger.error(f"No metrics found in stats file {stats_file}")
        if len(progress_rows) == 0:
            logger.error(f"No PROGRESS data found in stats file {stats_file}")
        return ret, progress_rows, timestamp

    def _parse_total_line(self, line: str) -> list[tuple[str, int]]:
        ret: list[tuple[str, int]] = []
        for match in self.pattern.finditer(line):
            metric_name, value = match.groups()
            ret.append((metric_name, int(value)))
        return ret

    def _parse_progress_lines(self, lines: Iterator[str]) -> list[ProgressRow]:
        if not next(lines).startswith("extsz"):
            logger.error("Unexpected format in PROGRESS section")
            return []
        if not next(lines).startswith("-----"):
            logger.error("Unexpected format in PROGRESS section")
            return []
        ret: list[BeesCollector.ProgressRow] = []
        for row in lines:
            parts = row.split()
            try:
                extsz = parts[0]
                if extsz == "total":
                    return ret
                if extsz not in ["max", "32M", "8M", "2M", "512K", "128K"]:
                    logger.error(f"Invalid extsz value: {extsz}")
                    continue
                datasz = parts[1]
                point = parts[2]
                gen_min = int(parts[3])
                gen_max = int(parts[4])
                if point != "idle":
                    point = int(point)  # type: ignore
                progress_row = self.ProgressRow(
                    extsz,  # type: ignore
                    datasz,
                    point,  # type: ignore
                    gen_min,
                    gen_max,
                )
                logger.debug(f"Parsed PROGRESS row: {progress_row}")
                ret.append(progress_row)
            except ValueError as e:
                logger.error(f"Error parsing PROGRESS row: {e}")
                continue
        return ret

    @staticmethod
    def _datasz_to_bytes(datasz: str) -> Optional[int]:
        units = {"K": 1024, "M": 1024**2, "G": 1024**3, "T": 1024**4}
        if datasz[-1] in units:
            return int(float(datasz[:-1]) * units[datasz[-1]])
        return None


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
    logger.info("Starting bees prometheus exporter")

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
