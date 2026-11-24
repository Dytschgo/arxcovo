use super::{Id, Refusal, Reply, Vault};
use ring::hmac;
use rusqlite::{OptionalExtension, Row, params};
use serde::Deserialize;
use serde_json::{Value, json};
use subtle::ConstantTimeEq;

pub(super) const VERSION_FIELDS: &str = "v.id,s.id,s.owner,s.os,s.name,v.hash,v.created_ms,
    CASE WHEN v.inconclusive=1 THEN 'test_inconclusive' ELSE v.state END,
    CASE WHEN v.inconclusive=1 THEN 'test_inconclusive' ELSE v.test_status END,
    v.exit_code,v.cleanup_confirmed";

pub(super) fn version(row: &Row<'_>) -> rusqlite::Result<Value> {
    let mut value = json!({
        "version_id": hex::encode(row.get::<_, Id>(0)?),
        "script_id": hex::encode(row.get::<_, Id>(1)?),
        "author": row.get::<_, String>(2)?,
        "os": row.get::<_, String>(3)?,
        "name": row.get::<_, String>(4)?,
        "sha256": hex::encode(row.get::<_, [u8; 32]>(5)?),
        "created_at_ms": row.get::<_, i64>(6)?,
        "state": row.get::<_, String>(7)?
    });
    if let Some(status) = row.get::<_, Option<String>>(8)? {
        let mut test = json!({"status": status, "cleanup_confirmed": row.get::<_, bool>(10)?});
        if let Some(exit_code) = row.get::<_, Option<u32>>(9)? {
            test["exit_code"] = json!(exit_code);
        }
        value["test"] = test;
    }
    Ok(value)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Page {
    #[serde(default = "default_limit")]
    limit: u8,
    cursor: Option<String>,
}

fn default_limit() -> u8 {
    20
}

pub(super) enum List {
    Scripts,
    Versions(Id),
    Audit,
}

impl List {
    fn cursor_domain(&self) -> Vec<u8> {
        match self {
            Self::Scripts => b"scripts".to_vec(),
            Self::Versions(id) => [b"versions".as_slice(), id].concat(),
            Self::Audit => b"audit".to_vec(),
        }
    }
}

impl Vault {
    pub(super) fn page(&self, owner: &str, list: List, page: Page) -> Reply {
        if !(1..=20).contains(&page.limit) {
            return Err(Refusal::Invalid);
        }
        let domain = list.cursor_domain();
        let before = match page.cursor {
            None => i64::MAX,
            Some(cursor) => self.read_cursor(owner, &domain, &cursor)?,
        };
        let fetch = i64::from(page.limit) + 1;
        let rows = self.store.read(|db| {
            let values = match list {
                List::Scripts => db.prepare(
                    "SELECT s.seq,s.id,s.os,s.name,v.id FROM scripts s
                     LEFT JOIN versions v ON v.script=s.seq AND v.state='active'
                     WHERE s.owner=?1 AND s.seq<?2 ORDER BY s.seq DESC LIMIT ?3",
                )?.query_map(params![owner, before, fetch], |r| {
                    let mut value = json!({
                        "script_id": hex::encode(r.get::<_, Id>(1)?),
                        "os": r.get::<_, String>(2)?,
                        "name": r.get::<_, String>(3)?
                    });
                    if let Some(active) = r.get::<_, Option<Id>>(4)? {
                        value["active_version_id"] = json!(hex::encode(active));
                    }
                    Ok((r.get(0)?, value))
                })?.collect::<rusqlite::Result<Vec<_>>>()?,
                List::Versions(script) => {
                    let seq: Option<i64> = db.query_row(
                        "SELECT seq FROM scripts WHERE owner=?1 AND id=?2",
                        params![owner, script], |r| r.get(0),
                    ).optional()?;
                    let Some(seq) = seq else { return Ok(None) };
                    db.prepare(&format!(
                        "SELECT {VERSION_FIELDS},v.seq FROM versions v JOIN scripts s ON s.seq=v.script
                         WHERE v.script=?1 AND v.seq<?2 ORDER BY v.seq DESC LIMIT ?3",
                    ))?.query_map(params![seq, before, fetch], |r| {
                        Ok((r.get(11)?, version(r)?))
                    })?.collect::<rusqlite::Result<Vec<_>>>()?
                }
                List::Audit => db.prepare(
                    "SELECT a.seq,a.action,a.at_ms,a.outcome,a.reason,a.version,a.operation,v.hash
                     FROM audit a LEFT JOIN versions v ON v.id=a.version
                     WHERE a.owner=?1 AND a.seq<?2 ORDER BY a.seq DESC LIMIT ?3",
                )?.query_map(params![owner, before, fetch], |r| {
                    let mut value = json!({
                        "tool": r.get::<_, String>(1)?,
                        "at_ms": r.get::<_, i64>(2)?,
                        "outcome": r.get::<_, String>(3)?
                    });
                    if let Some(reason) = r.get::<_, Option<String>>(4)? {
                        value["reason"] = json!(reason);
                    }
                    for (col, name) in [(5, "version_id"), (6, "operation_id")] {
                        if let Some(id) = r.get::<_, Option<Id>>(col)? {
                            value[name] = json!(hex::encode(id));
                        }
                    }
                    if let Some(hash) = r.get::<_, Option<[u8; 32]>>(7)? {
                        value["sha256"] = json!(hex::encode(hash));
                    }
                    Ok((r.get(0)?, value))
                })?.collect::<rusqlite::Result<Vec<_>>>()?,
            };
            Ok(Some(values))
        }).map_err(|_| Refusal::StateUnavailable)?;
        let Some(rows) = rows else {
            return Err(Refusal::NotFound);
        };
        let limit = usize::from(page.limit);
        let next = (rows.len() > limit).then(|| self.cursor(owner, &domain, rows[limit - 1].0));
        let entries: Vec<_> = rows
            .into_iter()
            .take(limit)
            .map(|(_, value)| value)
            .collect();
        let field = if matches!(list, List::Audit) {
            "activities"
        } else {
            "entries"
        };
        let mut result = json!({(field): entries});
        if let Some(next) = next {
            result["next_cursor"] = json!(next);
        }
        Ok(result)
    }

    fn tag(&self, owner: &str, domain: &[u8], payload: &[u8]) -> hmac::Tag {
        let mut context = hmac::Context::with_key(&self.cursor_key);
        context.update(b"arxcovo vault cursor v1\0");
        context.update(owner.as_bytes());
        context.update(&[0]);
        context.update(domain);
        context.update(payload);
        context.sign()
    }

    fn cursor(&self, owner: &str, domain: &[u8], before: i64) -> String {
        let mut bytes = [0; 41];
        bytes[0] = 1;
        bytes[1..9].copy_from_slice(&before.to_be_bytes());
        let tag = self.tag(owner, domain, &bytes[..9]);
        bytes[9..].copy_from_slice(tag.as_ref());
        hex::encode(bytes)
    }

    fn read_cursor(&self, owner: &str, domain: &[u8], cursor: &str) -> Result<i64, Refusal> {
        let mut bytes = [0; 41];
        if cursor.len() != 82
            || cursor
                .bytes()
                .any(|b| !matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Refusal::InvalidCursor);
        }
        hex::decode_to_slice(cursor, &mut bytes).map_err(|_| Refusal::InvalidCursor)?;
        let tag = self.tag(owner, domain, &bytes[..9]);
        if bytes[0] != 1 || !bool::from(tag.as_ref().ct_eq(&bytes[9..])) {
            return Err(Refusal::InvalidCursor);
        }
        let before = i64::from_be_bytes(bytes[1..9].try_into().expect("fixed cursor width"));
        if before <= 0 {
            return Err(Refusal::InvalidCursor);
        }
        Ok(before)
    }
}
