use crate::{cgroup, oom, procfs, util};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Critical,
    #[default]
    Unknown,
    Error,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Critical => "CRIT",
            Self::Unknown => "UNKNOWN",
            Self::Error => "ERROR",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub id: String,
    #[serde(deserialize_with = "required_nullable_number")]
    pub value: Option<f64>,
    pub unit: String,
}

// The protocol requires `value` to be present, while allowing an explicit null.
fn required_nullable_number<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<f64>, D::Error> {
    Option::<f64>::deserialize(deserializer)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub schema_version: u32,
    pub id: String,
    pub status: Status,
    pub summary: String,
    pub metrics: Vec<Metric>,
    pub source: String,
    pub updated_at: String,
    pub age_seconds: f64,
}
impl CheckResult {
    pub fn new(
        id: &str,
        status: Status,
        summary: String,
        metrics: Vec<Metric>,
        source: &str,
    ) -> Self {
        Self {
            schema_version: 1,
            id: id.into(),
            status,
            summary,
            metrics,
            source: source.into(),
            updated_at: util::timestamp(util::unix_now(), true),
            age_seconds: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Limit {
    pub value: Option<f64>,
    pub source: String,
    pub accounting: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub cpu_cores: f64,
    pub cpu_source: String,
    pub memory_bytes: u64,
    pub memory_source: String,
    pub cpu_limit: Limit,
    pub memory_limit: Limit,
    pub processes: procfs::Totals,
    pub throttled_periods: Option<u64>,
    pub periods: Option<u64>,
    pub throttle_ms: Option<f64>,
    pub oom_new: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub version: String,
    pub sample_id: u64,
    pub timestamp: String,
    pub unix_seconds: i64,
    pub host: String,
    pub user: util::User,
    pub sample_seconds: f64,
    pub collection_ms: f64,
    pub summary: Summary,
    pub account: cgroup::Stats,
    pub cpu_cgroup: Option<String>,
    pub memory_cgroup: Option<String>,
    pub processes: BTreeMap<u32, procfs::Process>,
    pub checks: Vec<CheckResult>,
    pub oom: oom::Log,
    pub warnings: Vec<String>,
}
