use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::enums::{CaseType, Difficulty};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProblemQueries {
    pub limit: Option<u64>,
    pub page: Option<u64>,
    pub query: Option<String>,
    pub difficulty: Option<Difficulty>,
    #[serde(default, deserialize_with = "deserialize_tag_option")]
    pub tag: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProblemResponseItem {
    pub id: Uuid,
    pub title: String,
    pub difficulty: Difficulty,
    pub tags: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListProblemResponse {
    pub problems: Vec<ListProblemResponseItem>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemStatResponse {
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemLimit {
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub memory_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDetailResponse {
    pub id: Uuid,
    pub author_id: Uuid,
    pub title: String,
    pub description: String,
    pub difficulty: Difficulty,
    pub tags: Vec<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub limit: ProblemLimit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseSuccessfulResponse {
    pub id: Uuid,
    pub input: String,
    pub output: String,
    #[serde(rename = "type")]
    pub case_type: CaseType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProblemRequest {
    pub title: String,
    pub description: String,
    pub difficulty: Difficulty,
    pub limit: ProblemLimit,
    pub tags: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProblemLimit {
    pub cpu_time_ms: Option<u64>,
    pub wall_time_ms: Option<u64>,
    pub memory_bytes: Option<u64>,
    pub output_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProblemRequest {
    pub title: Option<String>,
    pub description: Option<String>,
    pub difficulty: Option<Difficulty>,
    pub limit: Option<UpdateProblemLimit>,
    pub tags: Option<Vec<Uuid>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceTestCasesRequest {
    pub cases: Vec<TestCaseInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestCaseInput {
    pub input: String,
    pub output: String,
    #[serde(rename = "type")]
    pub case_type: CaseType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProblemResponse {
    pub id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SubmissionResult {
    Pending,
    Completed(judge_core_shared::models::http::VerdictResponse),
}

/// ### Accepts
/// - Absent key => [`None`]
/// - Single value (`?tag=id`) => [`Some(vec![id])`]
/// - Comma-separated values (`?tag=id1,id2`) => [`Some(vec![id1, id2])`]
fn deserialize_tag_option<'de, D>(deserializer: D) -> Result<Option<Vec<Uuid>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct TagVisitor;

    impl<'de> serde::de::Visitor<'de> for TagVisitor {
        type Value = Option<Vec<Uuid>>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a UUID or comma-separated list of UUIDs")
        }

        fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            let uuids: Vec<Uuid> = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Uuid::parse_str(s).map_err(E::custom))
                .collect::<Result<_, _>>()?;
            if uuids.is_empty() { Ok(None) } else { Ok(Some(uuids)) }
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut vec = Vec::new();
            while let Some(val) = seq.next_element::<&str>()? {
                if !val.is_empty() {
                    vec.push(Uuid::parse_str(val).map_err(serde::de::Error::custom)?);
                }
            }
            if vec.is_empty() { Ok(None) } else { Ok(Some(vec)) }
        }
    }

    deserializer.deserialize_any(TagVisitor)
}
