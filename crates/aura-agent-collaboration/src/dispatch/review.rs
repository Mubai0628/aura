use super::*;

pub fn policy(db: &Connection) -> StoreResult<PeerReviewPolicy> {
    Ok(db
        .query_row(
            "SELECT required,revision FROM agent_collaboration_review_settings WHERE id=1",
            [],
            |r| {
                Ok(PeerReviewPolicy {
                    required: r.get(0)?,
                    revision: u64::from(r.get::<_, u32>(1)?),
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn set_policy(tx: &Transaction<'_>, revision: u64, required: bool) -> StoreResult<()> {
    let current = policy(tx)?;
    if current.revision != revision {
        return Err(Error::Conflict);
    }
    if current.required != required {
        tx.execute("INSERT INTO agent_collaboration_review_settings VALUES(1,1,?1) ON CONFLICT(id) DO UPDATE SET required=excluded.required,revision=revision+1", [required])?;
    }
    Ok(())
}

pub(super) fn enqueue(tx: &Transaction<'_>, id: EventId) -> StoreResult<()> {
    if policy(tx)?.required {
        tx.execute(
            "INSERT INTO agent_collaboration_reviews(delivery_id) VALUES (?1)",
            [id.to_string()],
        )?;
    }
    Ok(())
}

pub fn waiting(db: &Connection, id: EventId) -> StoreResult<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM agent_collaboration_reviews WHERE delivery_id=?1 AND reviewed_by IS NULL)", [id.to_string()], |r| r.get(0))?)
}

pub fn revised_text(db: &Connection, id: EventId) -> StoreResult<Option<String>> {
    Ok(db.query_row("SELECT revised_text FROM agent_collaboration_reviews WHERE delivery_id=?1 AND reviewed_by IS NOT NULL", [id.to_string()], |r| r.get::<_, Option<String>>(0)).optional()?.flatten())
}

pub fn release(
    tx: &Transaction<'_>,
    delivery: &Delivery,
    session: SessionId,
    text: Option<&str>,
    at: UtcTimestamp,
) -> StoreResult<()> {
    if delivery.target != session || delivery.cancelled || delivery.target_run.is_some() {
        return Err(Error::Conflict);
    }
    if text.is_some_and(|s| s.trim().is_empty() || s.len() > MAX_PEER_TASK) {
        return Err(Error::Invalid("peers.review_invalid: 请填写有界的接收说明。"));
    }
    if tx.execute("UPDATE agent_collaboration_reviews SET reviewed_by=?2,revised_text=?3,reviewed_at=?4 WHERE delivery_id=?1 AND reviewed_by IS NULL", params![delivery.id.to_string(),session.to_string(),text,at.to_string()])? != 1 {
        return Err(Error::Conflict);
    }
    Ok(())
}
