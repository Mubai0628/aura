//! Whether the model a step ran on is sent images (its frozen request options), for a tool
//! that returns one to say whether the model will see it.
use super::*;
use aura_domain_common::ModelStepId;

/// False for a step without a snapshot, or one frozen before images were sent.
pub fn step_takes_images(db: &Connection, step: ModelStepId) -> StoreResult<bool> {
    let image_input: Option<i64> = db
        .query_row(
            "SELECT json_extract(s.payload,'$.request_options.image_input') FROM model_steps m              JOIN model_execution_snapshots s ON s.id=m.snapshot_id WHERE m.id=?1",
            [step.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(image_input == Some(1))
}
