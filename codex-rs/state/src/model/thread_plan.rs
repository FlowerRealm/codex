use anyhow::Result;
use chrono::DateTime;
use chrono::Utc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPlanItemStatus {
    Pending,
    InProgress,
    Completed,
}

impl ThreadPlanItemStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            _ => Err(anyhow::anyhow!("invalid thread plan item status: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPlanSnapshot {
    pub id: String,
    pub thread_id: String,
    pub source_turn_id: String,
    pub source_item_id: String,
    pub raw_csv: String,
    pub created_at: DateTime<Utc>,
    pub superseded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPlanItem {
    pub snapshot_id: String,
    pub row_id: String,
    pub row_index: i64,
    pub status: ThreadPlanItemStatus,
    pub step: String,
    pub path: String,
    pub details: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub depends_on: Vec<String>,
    pub acceptance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveThreadPlan {
    pub snapshot: ThreadPlanSnapshot,
    pub items: Vec<ThreadPlanItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPlanSnapshotCreateParams {
    pub id: String,
    pub thread_id: String,
    pub source_turn_id: String,
    pub source_item_id: String,
    pub raw_csv: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadPlanItemCreateParams {
    pub row_id: String,
    pub row_index: i64,
    pub status: ThreadPlanItemStatus,
    pub step: String,
    pub path: String,
    pub details: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub depends_on: Vec<String>,
    pub acceptance: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub(crate) struct ThreadPlanSnapshotRow {
    pub(crate) id: String,
    pub(crate) thread_id: String,
    pub(crate) source_turn_id: String,
    pub(crate) source_item_id: String,
    pub(crate) raw_csv: String,
    pub(crate) created_at: i64,
    pub(crate) superseded_at: Option<i64>,
}

impl TryFrom<ThreadPlanSnapshotRow> for ThreadPlanSnapshot {
    type Error = anyhow::Error;

    fn try_from(value: ThreadPlanSnapshotRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: value.id,
            thread_id: value.thread_id,
            source_turn_id: value.source_turn_id,
            source_item_id: value.source_item_id,
            raw_csv: value.raw_csv,
            created_at: epoch_seconds_to_datetime(value.created_at)?,
            superseded_at: value
                .superseded_at
                .map(epoch_seconds_to_datetime)
                .transpose()?,
        })
    }
}

fn epoch_seconds_to_datetime(secs: i64) -> Result<DateTime<Utc>> {
    DateTime::<Utc>::from_timestamp(secs, 0)
        .ok_or_else(|| anyhow::anyhow!("invalid unix timestamp: {secs}"))
}
