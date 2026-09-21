//! Applying a client's document to the registry, one ledger entry per row.
//!
//! Sections outside [`CLIENT_PUSH_SECTIONS`] are read, never written.

use axum::{Json, extract::State};
use sea_orm::TransactionTrait;
use std::collections::HashMap;
use utoipa::ToSchema;

use super::ledger::{Decision, Incoming, Status, apply_device_row};
use super::rows::Image;
use super::schema::{CLIENT_PUSH_SECTIONS, TABLES, normalise, table_for_section};
use crate::common::AppState;
use crate::common::auth::{AuthContext, Origin};
use crate::common::contract::ClientContract;
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct PushRequest {
    /// Contract version this document was built against, which must be the one negotiated
    /// for the exchange.
    pub contract_version: u32,
    /// Rows keyed by section name, as listed in `contract/sync-contract.json`. From
    /// contract 2 a row may carry `base_seq`, the `server_seq` the device last saw for it.
    #[schema(value_type = Object)]
    pub sections: HashMap<String, Vec<serde_json::Value>>,
}

/// A row that landed, and the position the device stores as its next `base_seq`.
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct Ack {
    pub id: uuid::Uuid,
    pub seq: i64,
}

/// A row that did not land, and why. The ledger keeps its values either way.
#[derive(Debug, Clone, serde::Serialize, ToSchema)]
pub struct Refusal {
    pub id: uuid::Uuid,
    pub reason: String,
}

#[derive(Debug, Default, serde::Serialize, ToSchema)]
pub struct SectionOutcome {
    pub received: usize,
    /// Rows written, inserted or merged.
    pub applied: Vec<Ack>,
    /// Rows another origin owns, or this origin has since moved past.
    pub superseded: Vec<Refusal>,
    /// Rows the console holds the last word on, kept as proposals a curator reviews.
    pub proposed: Vec<Refusal>,
    /// Rows the database would not take. Re-sending the same row cannot help.
    pub rejected: Vec<Refusal>,
}

impl SectionOutcome {
    fn absorb(&mut self, id: uuid::Uuid, decision: &Decision) {
        let reason = decision.reason.unwrap_or("").to_string();
        match decision.status {
            Status::Applied => self.applied.push(Ack {
                id,
                seq: decision.projected_seq.unwrap_or(decision.seq),
            }),
            Status::Superseded => self.superseded.push(Refusal { id, reason }),
            Status::Proposed | Status::Dismissed => self.proposed.push(Refusal { id, reason }),
            Status::Rejected => self.rejected.push(Refusal { id, reason }),
        }
    }
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PushResponse {
    /// Version agreed for this exchange, which is what the document had to declare.
    pub contract_version: u32,
    /// Sequence position after this push, for the next pull's `since`.
    pub cursor: i64,
    /// One [`SectionOutcome`] per section sent. Under contract 1 the older shape:
    /// `applied` a count, and `skipped`, `refused`, `conflicted` lists of ids.
    #[schema(value_type = Object)]
    pub sections: HashMap<String, serde_json::Value>,
}

/// Apply a document of survey metadata.
///
/// One transaction, sections in foreign-key order, so a client need not order its
/// own writes.
#[utoipa::path(
    post,
    path = "/sync/push",
    request_body = PushRequest,
    responses(
        (status = 200, description = "Document applied", body = PushResponse),
        (status = 400, description = "Unknown section, bad contract version, or malformed row"),
    ),
    tag = "sync"
)]
pub async fn push(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    axum::Extension(contract): axum::Extension<ClientContract>,
    Json(body): Json<PushRequest>,
) -> AppResult<Json<PushResponse>> {
    // The agreed version, not this server's maximum, so a client that negotiated down to an
    // older one is still writing a document the server asked it for.
    let agreed = contract.agreed();
    if body.contract_version != agreed {
        return Err(AppError::BadRequest(format!(
            "Unsupported contract_version {}: this exchange agreed on {agreed}",
            body.contract_version
        )));
    }

    // Before writing anything: applying the rest would silently drop rows the client
    // believes it handed over.
    for section in body.sections.keys() {
        if table_for_section(section).is_none() {
            return Err(AppError::BadRequest(format!("Unknown section: {section}")));
        }
    }

    // The router keeps a person off this route; this is the backstop.
    let device_id = match auth.origin() {
        Origin::Device { device_id, .. } => device_id,
        Origin::Human { .. } => {
            return Err(AppError::Forbidden(
                "Pushing is for enrolled devices".to_string(),
            ));
        }
    };

    let txn = state.db.begin().await?;
    let mut outcomes: HashMap<String, serde_json::Value> = HashMap::new();

    for spec in TABLES {
        let Some(rows) = body.sections.get(spec.section) else {
            continue;
        };
        let mut outcome = SectionOutcome {
            received: rows.len(),
            ..Default::default()
        };

        for row in rows {
            let incoming = read_row(spec, agreed, row)?;
            let id = incoming.id;
            // Read, never written: ancestors of what the device does author.
            if !CLIENT_PUSH_SECTIONS.contains(&spec.section) {
                outcome.proposed.push(Refusal {
                    id,
                    reason: "read_only_section".to_string(),
                });
                continue;
            }
            let decision = apply_device_row(&txn, spec, agreed, device_id, incoming).await?;
            outcome.absorb(id, &decision);
        }

        let rendered =
            serde_json::to_value(&outcome).map_err(|e| AppError::Internal(e.to_string()))?;
        outcomes.insert(spec.section.to_string(), rendered);
    }

    let cursor = super::current_cursor(&txn).await?;
    txn.commit().await?;

    Ok(Json(PushResponse {
        contract_version: agreed,
        cursor,
        sections: outcomes,
    }))
}

/// Type one row of a section into the image the ledger diffs and projects. Keys outside
/// the contract at `agreed` are ignored.
fn read_row(
    spec: &super::schema::TableSpec,
    agreed: u32,
    row: &serde_json::Value,
) -> AppResult<Incoming> {
    let object = row.as_object().ok_or_else(|| {
        AppError::BadRequest(format!("{}: each row must be an object", spec.section))
    })?;
    let id = object
        .get("id")
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<uuid::Uuid>().ok())
        .ok_or_else(|| {
            AppError::BadRequest(format!("{}: every row needs a UUID id", spec.section))
        })?;

    let base_seq = match object.get("base_seq") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(value.as_i64().ok_or_else(|| {
            AppError::BadRequest(format!(
                "{} {id}: base_seq must be an integer",
                spec.section
            ))
        })?),
    };

    let mut image = Image::new();
    for column in spec.writable_at(agreed) {
        // Provenance comes from the credential, never the payload.
        if column.name == "device_id" {
            continue;
        }
        let value = normalise(&column, object.get(column.name)).map_err(|e| match e {
            AppError::BadRequest(msg) => {
                AppError::BadRequest(format!("{} {id}: {msg}", spec.section))
            }
            other => other,
        })?;
        image.insert(column.name.to_string(), value);
    }
    Ok(Incoming {
        id,
        base_seq,
        image,
    })
}
