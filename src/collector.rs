use anyhow::{Context, Result};
use glob::glob;
use log::{debug, error};
use prometheus_client::collector::Collector;
use prometheus_client::encoding::{DescriptorEncoder, EncodeLabelSet, EncodeMetric};
use prometheus_client::metrics::counter::ConstCounter;
use prometheus_client::metrics::gauge::ConstGauge;
use regex::Regex;
use std::collections::BTreeMap;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::fs::{File, metadata};
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

#[derive(Debug, Clone)]
enum PointValue {
    Number(u64),
    Idle,
}

#[derive(Debug, Clone)]
struct ProgressRow {
    extsz: String,
    datasz: u64,
    point: PointValue,
    gen_min: u64,
    gen_max: u64,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct UuidLabel {
    uuid: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, EncodeLabelSet)]
struct UuidExtentLabel {
    uuid: String,
    extent_size: String,
}

#[derive(Debug)]
struct FsMetrics {
    stats: BTreeMap<String, f64>,
    progress: Vec<ProgressRow>,
    // Adding timestamps to metrics is currently not supported in the Rust client
    // See https://github.com/prometheus/client_rust/issues/126
    #[allow(unused)]
    timestamp: u64,
}

#[derive(Debug)]
enum ParserState {
    None,
    Total,
    Rates,
    Progress,
}

#[derive(Debug)]
pub struct BeesCollector {
    stats_dir: PathBuf,
    pattern: Regex,
}

impl BeesCollector {
    pub async fn new(stats_dir: PathBuf) -> Result<Self> {
        // Verify directory exists and is accessible
        metadata(&stats_dir)
            .await
            .with_context(|| format!("Cannot access stats directory: {:?}", stats_dir))?;

        let pattern =
            Regex::new(r"(?-u:(\w+)=(\d+))").context("Failed to compile regex pattern")?;

        Ok(BeesCollector { stats_dir, pattern })
    }

    /// Collect all data from bees status files
    async fn collect_all_data(&self) -> Result<BTreeMap<Uuid, FsMetrics>> {
        let status_file_pattern = format!("{}/*.status", self.stats_dir.display());
        let mut values: BTreeMap<Uuid, FsMetrics> = BTreeMap::new();

        for entry in glob(&status_file_pattern)
            .context("Failed to create glob pattern")?
            .filter_map(Result::ok)
        {
            if let Some(uuid) = entry
                .file_stem()
                .and_then(|s| Uuid::try_parse_ascii(s.as_bytes()).ok())
            {
                match self.collect_stats_from_file(&entry).await {
                    Ok(stats) => {
                        values.insert(uuid, stats);
                    }
                    Err(e) => {
                        error!("Failed to collect stats from {}: {}", entry.display(), e);
                    }
                }
            } else {
                error!("Failed to parse UUID from filename: {}", entry.display());
            }
        }

        Ok(values)
    }

    async fn collect_stats_from_file(&self, stats_file: &Path) -> Result<FsMetrics> {
        let file = File::open(stats_file)
            .await
            .with_context(|| format!("Cannot open stats file: {:?}", stats_file))?;

        let metadata = file
            .metadata()
            .await
            .context("Failed to get file metadata")?;

        let timestamp = metadata
            .modified()
            .context("Failed to get file modification time")?
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Failed to convert time to timestamp")?
            .as_secs();

        debug!("Reading stats from {:?}", stats_file);

        let reader = BufReader::new(file);
        let mut lines = reader.lines();
        let mut file_lines = Vec::new();

        while let Some(line) = lines
            .next_line()
            .await
            .context("Failed to read line from file")?
        {
            file_lines.push(line);
        }

        let mut stats: BTreeMap<String, f64> = BTreeMap::new();
        let mut progress: Vec<ProgressRow> = Vec::new();
        let mut parser_state = ParserState::None;
        let mut line_iter = file_lines.iter();

        while let Some(line) = line_iter.next() {
            if line.starts_with("TOTAL:") {
                parser_state = ParserState::Total;
                continue;
            }
            if line.starts_with("RATES:") {
                parser_state = ParserState::Rates;
                continue;
            }
            if line.starts_with("PROGRESS:") {
                parser_state = ParserState::Progress;
                progress = self.parse_progress_lines(&mut line_iter)?;
                continue;
            }

            match parser_state {
                ParserState::Rates | ParserState::None => continue,
                ParserState::Total => {
                    match self.parse_total_line(line) {
                        Ok(parsed_metrics) => {
                            stats.extend(parsed_metrics);
                        }
                        Err(e) => {
                            error!("Failed to parse TOTAL line '{}': {}", line, e);
                            // Continue processing other lines despite this error
                        }
                    }
                }
                ParserState::Progress => {
                    // Progress parsing is handled above when we encounter "PROGRESS:"
                }
            }
        }

        if stats.is_empty() {
            error!("No metrics found in stats file {:?}", stats_file);
        }
        if progress.is_empty() {
            error!("No PROGRESS data found in stats file {:?}", stats_file);
        }

        Ok(FsMetrics {
            stats,
            progress,
            timestamp,
        })
    }

    fn parse_total_line(&self, line: &str) -> Result<Vec<(String, f64)>> {
        let mut ret = Vec::new();
        for caps in line
            .split_ascii_whitespace()
            .filter_map(|word| self.pattern.captures(word))
        {
            let metric_name = caps
                .get(1)
                .context("Failed to capture metric name from regex")?
                .as_str()
                .to_string();
            let value: f64 = caps
                .get(2)
                .context("Failed to capture metric value from regex")?
                .as_str()
                .parse()
                .with_context(|| {
                    format!(
                        "Failed to parse metric value: {}",
                        caps.get(0).unwrap().as_str()
                    )
                })?;
            ret.push((metric_name, value));
        }
        if ret.is_empty() {
            return Err(anyhow::anyhow!(
                "No metrics parsed from TOTAL line: {}",
                line
            ));
        }
        Ok(ret)
    }

    fn parse_progress_lines(
        &self,
        lines: &mut std::slice::Iter<String>,
    ) -> Result<Vec<ProgressRow>> {
        // Check for header line
        if let Some(line) = lines.next() {
            if !line.starts_with("extsz") {
                return Err(anyhow::anyhow!(
                    "Unexpected format in PROGRESS section: expected header starting with 'extsz'"
                ));
            }
        } else {
            return Err(anyhow::anyhow!("Missing header in PROGRESS section"));
        }

        // Check for separator line
        if let Some(line) = lines.next() {
            if !line.starts_with("-----") {
                return Err(anyhow::anyhow!(
                    "Unexpected format in PROGRESS section: expected separator line"
                ));
            }
        } else {
            return Err(anyhow::anyhow!("Missing separator in PROGRESS section"));
        }

        let mut ret = Vec::new();

        for line in lines {
            let parts: Vec<&str> = line.split_ascii_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }

            let extsz = parts[0];
            if extsz == "total" {
                return Ok(ret);
            }

            if !["max", "32M", "8M", "2M", "512K", "128K"].contains(&extsz) {
                error!("Invalid extsz value: {}", extsz);
                continue;
            }

            let datasz = Self::datasz_to_bytes(parts[1])?;
            let point_str = parts[2];

            let point = if point_str == "idle" {
                PointValue::Idle
            } else {
                match point_str.parse::<u64>() {
                    Ok(val) => PointValue::Number(val),
                    Err(_) => {
                        error!("Error parsing point value: {}", point_str);
                        continue;
                    }
                }
            };

            let gen_min: u64 = match parts[3].parse() {
                Ok(val) => val,
                Err(_) => {
                    error!("Error parsing gen_min: {}", parts[3]);
                    continue;
                }
            };

            let gen_max: u64 = match parts[4].parse() {
                Ok(val) => val,
                Err(_) => {
                    error!("Error parsing gen_max: {}", parts[4]);
                    continue;
                }
            };

            let progress_row = ProgressRow {
                extsz: extsz.to_string(),
                datasz,
                point,
                gen_min,
                gen_max,
            };

            debug!("Parsed PROGRESS row: {:?}", progress_row);
            ret.push(progress_row);
        }

        Ok(ret)
    }

    fn datasz_to_bytes(datasz: &str) -> Result<u64> {
        if datasz.is_empty() {
            return Err(anyhow::anyhow!("Empty datasz string"));
        }

        let last_char = datasz
            .chars()
            .last()
            .ok_or(anyhow::anyhow!("Failed to get last char"))?;
        let multiplier = match last_char {
            'K' => 1024,
            'M' => 1024_u64.pow(2),
            'G' => 1024_u64.pow(3),
            'T' => 1024_u64.pow(4),
            _ => return Err(anyhow::anyhow!("Invalid datasz suffix")),
        };

        let number_part = &datasz[..datasz.len() - 1];
        let number: f64 = number_part.parse()?;
        Ok((number * multiplier as f64) as u64)
    }
}

impl Collector for BeesCollector {
    fn encode(&self, mut encoder: DescriptorEncoder) -> Result<(), std::fmt::Error> {
        // Collect all data from bees status files
        let values = match tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.collect_all_data())
        }) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to collect metrics: {}", e);
                return Ok(()); // Don't fail the encoding, just skip metrics
            }
        };

        // Group metrics by type to encode descriptors properly
        let mut stats_counters: BTreeMap<String, Vec<(UuidLabel, f64)>> = BTreeMap::new();
        let mut datasz_gauges: Vec<(UuidExtentLabel, i64)> = Vec::new();
        let mut point_gauges: Vec<(UuidExtentLabel, i64)> = Vec::new();
        let mut point_idle_gauges: Vec<(UuidExtentLabel, i64)> = Vec::new();
        let mut gen_min_gauges: Vec<(UuidExtentLabel, i64)> = Vec::new();
        let mut gen_max_gauges: Vec<(UuidExtentLabel, i64)> = Vec::new();

        // Process collected data and group by metric type
        for (uuid, fs_metrics) in values {
            // Group stats counters by metric name
            for (metric_name, value) in fs_metrics.stats {
                let label = UuidLabel {
                    uuid: uuid.as_hyphenated().to_string(),
                };
                stats_counters
                    .entry(metric_name.clone())
                    .or_default()
                    .push((label, value));

                debug!(
                    "Adding metric {} with value {} for uuid {}",
                    metric_name, value, uuid
                );
            }

            // Group progress metrics
            for progress_row in fs_metrics.progress {
                let label = UuidExtentLabel {
                    uuid: uuid.as_hyphenated().to_string(),
                    extent_size: progress_row.extsz.clone(),
                };

                datasz_gauges.push((label.clone(), progress_row.datasz as i64));

                // Handle point and idle
                match progress_row.point {
                    PointValue::Idle => {
                        point_idle_gauges.push((label.clone(), 1));
                    }
                    PointValue::Number(point_val) => {
                        point_idle_gauges.push((label.clone(), 0));
                        point_gauges.push((label.clone(), point_val as i64));
                    }
                }

                // Handle gen_min and gen_max
                gen_min_gauges.push((label.clone(), progress_row.gen_min as i64));
                gen_max_gauges.push((label, progress_row.gen_max as i64));
            }
        }

        // Encode stats counters
        for (metric_name, label_values) in stats_counters {
            let metric_registry_name = format!("bees_{}", metric_name.to_lowercase());
            let description = format!("Bees metric {}", metric_name);

            let mut metric_encoder = encoder.encode_descriptor(
                &metric_registry_name,
                &description,
                None,
                prometheus_client::metrics::MetricType::Counter,
            )?;

            for (label, value) in label_values {
                let counter = ConstCounter::new(value);
                let sample_encoder = metric_encoder.encode_family(&label)?;
                counter.encode(sample_encoder)?;
            }
        }

        // Encode progress summary gauges
        if !datasz_gauges.is_empty() {
            let mut metric_encoder = encoder.encode_descriptor(
                "bees_progress_summary_datasz_bytes",
                "Bees progress summary datasz in bytes",
                None,
                prometheus_client::metrics::MetricType::Gauge,
            )?;
            for (label, value) in datasz_gauges {
                let gauge = ConstGauge::new(value);
                let sample_encoder = metric_encoder.encode_family(&label)?;
                gauge.encode(sample_encoder)?;
            }
        }

        if !point_gauges.is_empty() {
            let mut metric_encoder = encoder.encode_descriptor(
                "bees_progress_summary_point",
                "Bees progress summary",
                None,
                prometheus_client::metrics::MetricType::Gauge,
            )?;
            for (label, value) in point_gauges {
                let gauge = ConstGauge::new(value);
                let sample_encoder = metric_encoder.encode_family(&label)?;
                gauge.encode(sample_encoder)?;
            }
        }

        if !point_idle_gauges.is_empty() {
            let mut metric_encoder = encoder.encode_descriptor(
                "bees_progress_summary_point_idle",
                "Bees progress summary idle",
                None,
                prometheus_client::metrics::MetricType::Gauge,
            )?;
            for (label, value) in point_idle_gauges {
                let gauge = ConstGauge::new(value);
                let sample_encoder = metric_encoder.encode_family(&label)?;
                gauge.encode(sample_encoder)?;
            }
        }

        if !gen_min_gauges.is_empty() {
            let mut metric_encoder = encoder.encode_descriptor(
                "bees_progress_summary_gen_min",
                "Bees progress summary gen_min",
                None,
                prometheus_client::metrics::MetricType::Gauge,
            )?;
            for (label, value) in gen_min_gauges {
                let gauge = ConstGauge::new(value);
                let sample_encoder = metric_encoder.encode_family(&label)?;
                gauge.encode(sample_encoder)?;
            }
        }

        if !gen_max_gauges.is_empty() {
            let mut metric_encoder = encoder.encode_descriptor(
                "bees_progress_summary_gen_max",
                "Bees progress summary gen_max",
                None,
                prometheus_client::metrics::MetricType::Gauge,
            )?;
            for (label, value) in gen_max_gauges {
                let gauge = ConstGauge::new(value);
                let sample_encoder = metric_encoder.encode_family(&label)?;
                gauge.encode(sample_encoder)?;
            }
        }

        Ok(())
    }
}
