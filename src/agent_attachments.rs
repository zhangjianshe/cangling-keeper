//! Immutable, account-scoped conversation attachments. Bytes never enter ordinary detail responses.
use crate::store::Store;
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

pub(crate) const MAX_ATTACHMENTS: usize = 4;
const MAX_IMAGE: usize = 4 * 1024 * 1024;
const MAX_TEXT: usize = 64 * 1024;
fn sql<T>(r: rusqlite::Result<T>) -> Result<T, String> {
    r.map_err(|e| e.to_string())
}

const IMMUTABLE_TRIGGER: &str = "CREATE TRIGGER IF NOT EXISTS agent_attachments_immutable BEFORE UPDATE ON agent_attachments BEGIN SELECT RAISE(ABORT,'attachments are immutable'); END;";
pub(crate) fn initialize(store: &Store) -> Result<(), String> {
    sql(store.conn.execute_batch("CREATE TABLE IF NOT EXISTS agent_attachments (
        id TEXT PRIMARY KEY, owner_key TEXT NOT NULL, session_id TEXT NOT NULL, name TEXT NOT NULL,
        mime_type TEXT NOT NULL, kind TEXT NOT NULL, data BLOB NOT NULL, sha256 TEXT NOT NULL, created_at INTEGER NOT NULL,
        size INTEGER, width INTEGER, height INTEGER);
        CREATE INDEX IF NOT EXISTS idx_agent_attachments_session ON agent_attachments(owner_key,session_id,created_at);
        CREATE TABLE IF NOT EXISTS agent_message_attachments (
        message_id TEXT NOT NULL, attachment_id TEXT NOT NULL, position INTEGER NOT NULL,
        PRIMARY KEY(message_id,attachment_id), UNIQUE(message_id,position));
        CREATE INDEX IF NOT EXISTS idx_agent_message_attachments_id ON agent_message_attachments(attachment_id);"))?;
    let columns = sql(sql(
        sql(store.conn.prepare("PRAGMA table_info(agent_attachments)"))?
            .query_map([], |r| r.get::<_, String>(1)),
    )?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    let missing: Vec<_> = ["size", "width", "height"]
        .into_iter()
        .filter(|name| !columns.iter().any(|column| column == name))
        .collect();
    let needs_backfill=!missing.is_empty() || sql(store.conn.query_row("SELECT EXISTS(SELECT 1 FROM agent_attachments WHERE size IS NULL OR (kind='image' AND (width IS NULL OR height IS NULL)))",[],|r|r.get::<_,bool>(0)))?;
    if needs_backfill {
        let tx = sql(store.conn.unchecked_transaction())?;
        // Legacy attachment rows are validated once while deriving immutable public metadata.
        // DDL and backfill share a transaction, so failures restore the original trigger/data.
        sql(store
            .conn
            .execute_batch("DROP TRIGGER IF EXISTS agent_attachments_immutable;"))?;
        for column in missing {
            sql(store.conn.execute_batch(&format!(
                "ALTER TABLE agent_attachments ADD COLUMN {column} INTEGER;"
            )))?;
        }
        let ids = {
            let mut stmt=sql(store.conn.prepare("SELECT id FROM agent_attachments WHERE size IS NULL OR (kind='image' AND (width IS NULL OR height IS NULL))"))?;
            sql(sql(stmt.query_map([], |r| r.get::<_, String>(0)))?
                .collect::<rusqlite::Result<Vec<_>>>())?
        };
        for id in ids {
            let (id, name, mime, kind, data, hash) = sql(store.conn.query_row(
                "SELECT id,name,mime_type,kind,data,sha256 FROM agent_attachments WHERE id=?1",
                [id],
                row,
            ))?;
            let canonical = validate(&name, &mime, &data)?;
            if canonical["kind"] != kind
                || canonical["mimeType"] != mime
                || canonical["sha256"] != hash
            {
                return Err("附件完整性校验失败".into());
            }
            sql(store.conn.execute(
                "UPDATE agent_attachments SET size=?2,width=?3,height=?4 WHERE id=?1",
                params![
                    id,
                    canonical["size"].as_u64().unwrap() as i64,
                    canonical["width"].as_u64().map(|n| n as i64),
                    canonical["height"].as_u64().map(|n| n as i64)
                ],
            ))?;
        }
        sql(store.conn.execute_batch(IMMUTABLE_TRIGGER))?;
        sql(tx.commit())?;
    } else {
        sql(store.conn.execute_batch(IMMUTABLE_TRIGGER))?;
    }
    Ok(())
}

fn be32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes[..4].try_into().unwrap())
}
fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}
fn le24(bytes: &[u8]) -> u32 {
    bytes[0] as u32 | (bytes[1] as u32) << 8 | (bytes[2] as u32) << 16
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}
fn dimensions_fit(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= 8192
        && height <= 8192
        && width as u64 * height as u64 <= 24_000_000
}
fn webp_dimensions(kind: &[u8], chunk: &[u8]) -> Result<(u32, u32), String> {
    match kind {
        b"VP8 " if chunk.len() >= 10 && chunk[0] & 1 == 0 && chunk[3..6] == [0x9d, 0x01, 0x2a] => {
            Ok((
                (u16::from_le_bytes(chunk[6..8].try_into().unwrap()) & 0x3fff) as u32,
                (u16::from_le_bytes(chunk[8..10].try_into().unwrap()) & 0x3fff) as u32,
            ))
        }
        b"VP8L" if chunk.len() >= 5 && chunk[0] == 0x2f && chunk[4] & 0xe0 == 0 => {
            let bits = le32(&chunk[1..]);
            Ok((1 + (bits & 0x3fff), 1 + ((bits >> 14) & 0x3fff)))
        }
        _ => Err("WebP 图片头无效".into()),
    }
}
// Header/container validation only: no image decoding or transcoding, and no unbounded allocation.
// PNG: https://www.w3.org/TR/png-3/ ; JPEG: https://www.w3.org/Graphics/JPEG/itu-t81.pdf Annex B;
// WebP: https://developers.google.com/speed/webp/docs/riff_container .
fn image_header(data: &[u8]) -> Result<Option<(&'static str, u32, u32)>, String> {
    let invalid = || "图片格式无效或文件不完整".to_owned();
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut offset = 8usize;
        let mut dims = None;
        let mut has_data = false;
        while offset.checked_add(12).is_some_and(|end| end <= data.len()) {
            let size = be32(&data[offset..]) as usize;
            let end = offset
                .checked_add(12)
                .and_then(|n| n.checked_add(size))
                .filter(|end| *end <= data.len())
                .ok_or_else(invalid)?;
            let kind = &data[offset + 4..offset + 8];
            let chunk = &data[offset + 8..end - 4];
            if crc32(&data[offset + 4..end - 4]) != be32(&data[end - 4..end]) {
                return Err(invalid());
            }
            if dims.is_none() {
                if kind != b"IHDR" || size != 13 {
                    return Err(invalid());
                }
                let bit = chunk[8];
                let color = chunk[9];
                if !matches!(
                    (color, bit),
                    (0, 1 | 2 | 4 | 8 | 16) | (2, 8 | 16) | (3, 1 | 2 | 4 | 8) | (4 | 6, 8 | 16)
                ) || chunk[10] != 0
                    || chunk[11] != 0
                    || chunk[12] > 1
                {
                    return Err(invalid());
                }
                dims = Some((be32(chunk), be32(&chunk[4..])));
            } else if kind == b"IHDR" {
                return Err(invalid());
            }
            if kind == b"fcTL" {
                if size != 26 {
                    return Err(invalid());
                }
                let (w, h) = dims.ok_or_else(invalid)?;
                let fw = be32(&chunk[4..]);
                let fh = be32(&chunk[8..]);
                let x = be32(&chunk[12..]);
                let y = be32(&chunk[16..]);
                if !dimensions_fit(fw, fh)
                    || x as u64 + fw as u64 > w as u64
                    || y as u64 + fh as u64 > h as u64
                {
                    return Err(invalid());
                }
            }
            if kind == b"IDAT" && size > 0 {
                has_data = true;
            }
            if kind == b"IEND" {
                if size != 0 || end != data.len() || !has_data {
                    return Err(invalid());
                }
                let (w, h) = dims.ok_or_else(invalid)?;
                return Ok(Some(("image/png", w, h)));
            }
            offset = end;
        }
        return Err(invalid());
    }
    if data.starts_with(b"\xff\xd8") {
        let mut offset = 2usize;
        let mut dims = None;
        while offset < data.len() {
            if data[offset] != 0xff {
                return Err(invalid());
            }
            while offset < data.len() && data[offset] == 0xff {
                offset += 1;
            }
            let marker = *data.get(offset).ok_or_else(invalid)?;
            offset += 1;
            if matches!(marker, 0x00 | 0xd8 | 0xd9) {
                return Err(invalid());
            }
            if matches!(marker, 0x01 | 0xd0..=0xd7) {
                continue;
            }
            let length = data.get(offset..offset + 2).ok_or_else(invalid)?;
            let length = u16::from_be_bytes(length.try_into().unwrap()) as usize;
            let end = offset
                .checked_add(length)
                .filter(|n| length >= 2 && *n <= data.len())
                .ok_or_else(invalid)?;
            if matches!(marker,0xc0..=0xc3|0xc5..=0xc7|0xc9..=0xcb|0xcd..=0xcf) {
                if length < 8 {
                    return Err(invalid());
                }
                let height =
                    u16::from_be_bytes(data[offset + 3..offset + 5].try_into().unwrap()) as u32;
                let width =
                    u16::from_be_bytes(data[offset + 5..offset + 7].try_into().unwrap()) as u32;
                let components = data[offset + 7] as usize;
                if components == 0 || length != 8 + 3 * components {
                    return Err(invalid());
                }
                dims = Some((width, height));
            }
            if marker == 0xda {
                // Scan data is opaque; require a complete terminal EOI after this bounded header.
                if length < 6
                    || data[offset + 2] == 0
                    || length != 6 + 2 * data[offset + 2] as usize
                    || end + 2 >= data.len()
                    || !data.ends_with(b"\xff\xd9")
                {
                    return Err(invalid());
                }
                let (w, h) = dims.ok_or_else(invalid)?;
                return Ok(Some(("image/jpeg", w, h)));
            }
            offset = end;
        }
        return Err(invalid());
    }
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP") {
        if data.len() < 20 || le32(&data[4..]) as u64 + 8 != data.len() as u64 {
            return Err(invalid());
        }
        let mut offset = 12usize;
        let mut dims = None;
        let mut has_data = false;
        let mut animated = false;
        while offset + 8 <= data.len() {
            let kind = &data[offset..offset + 4];
            let size = le32(&data[offset + 4..]) as usize;
            let end = offset
                .checked_add(8)
                .and_then(|n| n.checked_add(size))
                .filter(|n| *n <= data.len())
                .ok_or_else(invalid)?;
            let chunk = &data[offset + 8..end];
            if offset == 12 && !matches!(kind, b"VP8X" | b"VP8 " | b"VP8L") {
                return Err(invalid());
            }
            match kind {
                b"VP8X" => {
                    if offset != 12
                        || size != 10
                        || chunk[0] & 0xc1 != 0
                        || chunk[1..4] != [0, 0, 0]
                    {
                        return Err(invalid());
                    }
                    dims = Some((1 + le24(&chunk[4..]), 1 + le24(&chunk[7..])));
                    animated = chunk[0] & 2 != 0;
                }
                b"VP8 " | b"VP8L" => {
                    let actual = webp_dimensions(kind, chunk)?;
                    if has_data
                        || animated
                        || !dimensions_fit(actual.0, actual.1)
                        || dims.is_some_and(|canvas| canvas != actual)
                    {
                        return Err(invalid());
                    }
                    dims = Some(actual);
                    has_data = true;
                }
                b"ANMF" => {
                    if !animated || size < 29 {
                        return Err(invalid());
                    }
                    let (cw, ch) = dims.ok_or_else(invalid)?;
                    let fw = 1 + le24(&chunk[6..]);
                    let fh = 1 + le24(&chunk[9..]);
                    let x = 2 * le24(chunk);
                    let y = 2 * le24(&chunk[3..]);
                    if chunk[15] & 0xfc != 0
                        || !dimensions_fit(fw, fh)
                        || x as u64 + fw as u64 > cw as u64
                        || y as u64 + fh as u64 > ch as u64
                    {
                        return Err(invalid());
                    }
                    let mut sub = 16usize;
                    let mut frame = false;
                    while sub + 8 <= size {
                        let subkind = &chunk[sub..sub + 4];
                        let len = le32(&chunk[sub + 4..]) as usize;
                        let subend = sub
                            .checked_add(8)
                            .and_then(|n| n.checked_add(len))
                            .filter(|n| *n <= size)
                            .ok_or_else(invalid)?;
                        if matches!(subkind, b"VP8 " | b"VP8L") {
                            if frame
                                || webp_dimensions(subkind, &chunk[sub + 8..subend])? != (fw, fh)
                            {
                                return Err(invalid());
                            }
                            frame = true;
                        } else if subkind != b"ALPH" || frame {
                            return Err(invalid());
                        }
                        sub = subend + (len & 1);
                        if sub > size || (len & 1 == 1 && chunk[subend] != 0) {
                            return Err(invalid());
                        }
                    }
                    if sub != size || !frame {
                        return Err(invalid());
                    }
                    has_data = true;
                }
                _ => {}
            }
            offset = end + (size & 1);
            if offset > data.len() || (size & 1 == 1 && data[end] != 0) {
                return Err(invalid());
            }
        }
        if offset != data.len() || !has_data {
            return Err(invalid());
        }
        let (w, h) = dims.ok_or_else(invalid)?;
        return Ok(Some(("image/webp", w, h)));
    }
    Ok(None)
}

/// Canonical metadata without an id. Used again when importing an untrusted backup.
pub(crate) fn validate(name: &str, mime: &str, data: &[u8]) -> Result<Value, String> {
    if name.is_empty()
        || name.trim() != name
        || name == "."
        || name == ".."
        || name.chars().count() > 128
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
    {
        return Err("附件名称须为最多128个字符的文件名，不能包含路径或控制字符".into());
    }
    if mime.len() > 128 || mime.chars().any(char::is_control) {
        return Err("附件类型无效".into());
    }
    if data.len() > MAX_IMAGE {
        return Err("每张图片不能超过4 MiB；文本附件不能超过64 KiB".into());
    }
    let declared = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let detected = image_header(data)?;
    let mut result = if let Some((actual, width, height)) = detected {
        if data.len() > MAX_IMAGE {
            return Err("每张图片不能超过4 MiB".into());
        }
        if !dimensions_fit(width, height) {
            return Err("图片尺寸最多8192×8192，且不能超过2400万像素".into());
        }
        if !declared.is_empty() && declared != "application/octet-stream" && declared != actual {
            return Err("图片内容与声明的文件类型不一致".into());
        }
        json!({"name":name,"mimeType":actual,"kind":"image","width":width,"height":height})
    } else {
        if declared.starts_with("image/") {
            return Err("仅支持有效的 PNG、JPEG 或 WebP 图片".into());
        }
        if data.len() > MAX_TEXT {
            return Err("每个文本附件不能超过64 KiB".into());
        }
        let text =
            std::str::from_utf8(data).map_err(|_| "附件须为 UTF-8 纯文本或 PNG/JPEG/WebP 图片")?;
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err("文本附件包含二进制或控制字符".into());
        }
        json!({"name":name,"mimeType":"text/plain","kind":"text"})
    };
    result["size"] = json!(data.len());
    result["sha256"] = json!(format!("{:x}", Sha256::digest(data)));
    Ok(result)
}
// Public polling reads these validated immutable columns, never the attachment BLOB.
fn metadata_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let kind = r.get::<_, String>(3)?;
    let mut value = json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"mimeType":r.get::<_,String>(2)?,"kind":kind,"size":r.get::<_,u32>(4)?,"sha256":r.get::<_,String>(5)?});
    if kind == "image" {
        value["width"] = json!(r.get::<_, u32>(6)?);
        value["height"] = json!(r.get::<_, u32>(7)?);
    }
    Ok(value)
}

fn row(
    r: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, String, String, String, Vec<u8>, String)> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
    ))
}
fn owned(store: &Store, session: &str) -> Result<String, String> {
    crate::agent_history::info(store, session)?;
    crate::agent_account::owner(store)
}
pub(crate) fn read_bytes(
    store: &Store,
    session: &str,
    id: &str,
) -> Result<(Value, Vec<u8>), String> {
    let owner = owned(store, session)?;
    let (id,name,mime,kind,data,hash)=sql(store.conn.query_row("SELECT id,name,mime_type,kind,data,sha256 FROM agent_attachments WHERE id=?1 AND session_id=?2 AND owner_key=?3",params![id,session,owner],row).optional())?.ok_or("附件不存在或不属于当前会话")?;
    let mut canonical = validate(&name, &mime, &data)?;
    if canonical["kind"] != kind || canonical["mimeType"] != mime || canonical["sha256"] != hash {
        return Err("附件完整性校验失败".into());
    }
    canonical["id"] = json!(id);
    Ok((canonical, data))
}
fn ids(value: &Value) -> Result<Vec<&str>, String> {
    let array = value.as_array().ok_or("attachmentIds 必须为数组")?;
    if array.len() > MAX_ATTACHMENTS {
        return Err("每条消息最多4个附件".into());
    }
    let mut unique = HashSet::new();
    let mut result = Vec::new();
    for id in array {
        let id = id
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or("附件 ID 无效")?;
        if !unique.insert(id) {
            return Err("附件不能重复".into());
        }
        result.push(id);
    }
    Ok(result)
}
pub(crate) fn message_ids(store: &Store, session: &str, message: &str) -> Result<Value, String> {
    let owner = owned(store, session)?;
    let mut stmt=sql(store.conn.prepare("SELECT a.id FROM agent_message_attachments l JOIN agent_attachments a ON a.id=l.attachment_id JOIN agent_messages m ON m.id=l.message_id AND m.session_id=a.session_id WHERE l.message_id=?1 AND a.session_id=?2 AND a.owner_key=?3 ORDER BY l.position"))?;
    Ok(json!(sql(sql(stmt
        .query_map(params![message, session, owner], |r| {
            r.get::<_, String>(0)
        }))?
    .collect::<rusqlite::Result<Vec<_>>>())?))
}
pub(crate) fn draft(store: &Store, session: &str) -> Result<Value, String> {
    let owner = owned(store, session)?;
    let mut stmt=sql(store.conn.prepare("SELECT id,name,mime_type,kind,size,sha256,width,height FROM agent_attachments a WHERE session_id=?1 AND owner_key=?2 AND NOT EXISTS(SELECT 1 FROM agent_message_attachments l WHERE l.attachment_id=a.id) ORDER BY created_at,rowid"))?;
    Ok(json!(sql(sql(
        stmt.query_map(params![session, owner], metadata_row)
    )?
    .collect::<rusqlite::Result<Vec<_>>>())?))
}
pub(crate) fn decorate_messages(
    store: &Store,
    session: &str,
    messages: &mut [Value],
) -> Result<(), String> {
    let owner = owned(store, session)?;
    let mut stmt=sql(store.conn.prepare("SELECT a.id,a.name,a.mime_type,a.kind,a.size,a.sha256,a.width,a.height FROM agent_message_attachments l JOIN agent_attachments a ON a.id=l.attachment_id JOIN agent_messages m ON m.id=l.message_id AND m.session_id=a.session_id WHERE l.message_id=?1 AND a.session_id=?2 AND a.owner_key=?3 ORDER BY l.position"))?;
    for message in messages {
        let message_id = message["id"].as_str().ok_or("消息缺少ID")?;
        message["attachments"] = json!(sql(sql(
            stmt.query_map(params![message_id, session, owner], metadata_row)
        )?
        .collect::<rusqlite::Result<Vec<_>>>())?);
    }
    Ok(())
}

pub(crate) fn context_message(
    store: &Store,
    session: &str,
    message: &Value,
) -> Result<Value, String> {
    let linked = message_ids(store, session, message["id"].as_str().ok_or("消息缺少ID")?)?;
    let mut result = json!({"role":message["role"],"content":message["content"]});
    let mut attachments = Vec::new();
    for id in ids(&linked)? {
        let (mut metadata, data) = read_bytes(store, session, id)?;
        if metadata["kind"] == "text" {
            metadata["text"] =
                json!(std::str::from_utf8(&data).map_err(|_| "已保存文本的编码无效")?);
        }
        attachments.push(metadata);
    }
    if !attachments.is_empty() {
        result["keeperAttachments"] = json!(attachments);
    }
    Ok(result)
}
/// Caller must hold the surrounding message transaction, so failed validation leaves every draft intact.
pub(crate) fn bind(
    store: &Store,
    session: &str,
    message: &str,
    attachments: &Value,
) -> Result<(), String> {
    owned(store, session)?;
    let selected = ids(attachments)?;
    let valid: bool = sql(store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_messages WHERE id=?1 AND session_id=?2 AND role='user')",
        params![message, session],
        |r| r.get(0),
    ))?;
    if !valid {
        return Err("用户消息不存在或不属于当前会话".into());
    }
    let existing: i64 = sql(store.conn.query_row(
        "SELECT count(*) FROM agent_message_attachments WHERE message_id=?1",
        [message],
        |r| r.get(0),
    ))?;
    if existing != 0 {
        return Err("消息附件已绑定，不能修改".into());
    }
    for id in &selected {
        read_bytes(store, session, id)?;
        let bound: bool = sql(store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_message_attachments WHERE attachment_id=?1)",
            [id],
            |r| r.get(0),
        ))?;
        if bound {
            return Err("附件已用于消息，不能重复绑定".into());
        }
    }
    for (position, id) in selected.iter().enumerate() {
        sql(store.conn.execute("INSERT INTO agent_message_attachments(message_id,attachment_id,position) VALUES (?1,?2,?3)",params![message,id,position as i64]))?;
    }
    Ok(())
}
pub(crate) fn clone_links(
    store: &Store,
    session: &str,
    old: &str,
    new: &str,
) -> Result<(), String> {
    owned(store, session)?;
    let valid: i64 = sql(store.conn.query_row(
        "SELECT count(*) FROM agent_messages WHERE id IN (?1,?2) AND session_id=?3 AND role='user'",
        params![old, new, session],
        |r| r.get(0),
    ))?;
    if valid != 2 {
        return Err("用户消息不存在或不属于当前会话".into());
    }
    let existing: bool = sql(store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_message_attachments WHERE message_id=?1)",
        [new],
        |r| r.get(0),
    ))?;
    if existing {
        return Err("新消息附件已绑定".into());
    }
    let linked = message_ids(store, session, old)?;
    for (position, id) in ids(&linked)?.iter().enumerate() {
        sql(store.conn.execute("INSERT INTO agent_message_attachments(message_id,attachment_id,position) VALUES (?1,?2,?3)",params![new,id,position as i64]))?;
    }
    Ok(())
}
pub(crate) fn ensure_supported(
    store: &Store,
    session: &str,
    attachments: &Value,
    config: &Value,
) -> Result<(), String> {
    for id in ids(attachments)? {
        let (metadata, _) = read_bytes(store, session, id)?;
        if metadata["kind"] == "image"
            && config["enabled"] == true
            && config["hasApiKey"] == true
            && config["supportsImages"] != true
        {
            return Err("当前模型未声明支持图片，请更换支持图片的模型或移除图片附件".into());
        }
    }
    Ok(())
}
pub(crate) fn upload(store: &Store, args: &Value) -> Result<Value, String> {
    let session = args["sessionId"].as_str().ok_or("缺少参数 sessionId")?;
    crate::agent_history::require_active(store, session)?;
    let owner = owned(store, session)?;
    let tx = sql(store.conn.unchecked_transaction())?;
    require_idle(store, session)?;
    let count:i64=sql(store.conn.query_row("SELECT count(*) FROM agent_attachments a WHERE owner_key=?1 AND session_id=?2 AND NOT EXISTS(SELECT 1 FROM agent_message_attachments l WHERE l.attachment_id=a.id)",params![owner,session],|r|r.get(0)))?;
    if count >= MAX_ATTACHMENTS as i64 {
        return Err("每条消息最多4个待发送附件，请先发送或移除附件".into());
    }
    let encoded = args["data"].as_str().ok_or("缺少附件数据")?;
    if encoded.len() > MAX_IMAGE.div_ceil(3) * 4 {
        return Err("每张图片不能超过4 MiB".into());
    }
    let data = STANDARD
        .decode(encoded)
        .map_err(|_| "附件数据不是有效的 base64")?;
    let mut metadata = validate(
        args["name"].as_str().ok_or("缺少附件名称")?,
        args["mimeType"].as_str().unwrap_or(""),
        &data,
    )?;
    let id = uuid::Uuid::new_v4().to_string();
    metadata["id"] = json!(id);
    sql(store.conn.execute("INSERT INTO agent_attachments(id,owner_key,session_id,name,mime_type,kind,data,sha256,created_at,size,width,height) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![id,owner,session,metadata["name"].as_str().unwrap(),metadata["mimeType"].as_str().unwrap(),metadata["kind"].as_str().unwrap(),data,metadata["sha256"].as_str().unwrap(),chrono::Utc::now().timestamp_millis(),metadata["size"].as_u64().unwrap() as i64,metadata["width"].as_u64().map(|n|n as i64),metadata["height"].as_u64().map(|n|n as i64)]))?;
    sql(tx.commit())?;
    Ok(metadata)
}
fn require_idle(store: &Store, session: &str) -> Result<(), String> {
    let busy: bool = sql(store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_model_pending WHERE session_id=?1)",
        [session],
        |r| r.get(0),
    ))?;
    if busy {
        Err("当前会话正在生成回复，请先停止或等待完成".into())
    } else {
        Ok(())
    }
}
pub(crate) fn content(store: &Store, args: &Value) -> Result<Value, String> {
    let (mut metadata, data) = read_bytes(
        store,
        args["sessionId"].as_str().ok_or("缺少参数 sessionId")?,
        args["id"].as_str().ok_or("缺少参数 id")?,
    )?;
    metadata["data"] = json!(STANDARD.encode(data));
    Ok(metadata)
}
pub(crate) fn delete(store: &Store, args: &Value) -> Result<Value, String> {
    let session = args["sessionId"].as_str().ok_or("缺少参数 sessionId")?;
    let id = args["id"].as_str().ok_or("缺少参数 id")?;
    crate::agent_history::require_active(store, session)?;
    let owner = owned(store, session)?;
    let tx = sql(store.conn.unchecked_transaction())?;
    require_idle(store, session)?;
    read_bytes(store, session, id)?;
    let deleted=sql(store.conn.execute("DELETE FROM agent_attachments WHERE id=?1 AND session_id=?2 AND owner_key=?3 AND NOT EXISTS(SELECT 1 FROM agent_message_attachments WHERE attachment_id=?1)",params![id,session,owner]))?;
    if deleted != 1 {
        return Err("已用于消息的附件不能删除".into());
    }
    sql(tx.commit())?;
    Ok(json!({"id":id,"deleted":true}))
}
pub(crate) fn purge(store: &Store, session: &str) -> Result<(), String> {
    let owner = owned(store, session)?;
    sql(store.conn.execute("DELETE FROM agent_message_attachments WHERE attachment_id IN (SELECT id FROM agent_attachments WHERE session_id=?1 AND owner_key=?2) OR message_id IN (SELECT id FROM agent_messages WHERE session_id=?1)",params![session,owner]))?;
    sql(store.conn.execute(
        "DELETE FROM agent_attachments WHERE session_id=?1 AND owner_key=?2",
        params![session, owner],
    ))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAADCAIAAAA2iEnWAAAAFUlEQVR4nGP8//8/AwMDEwMDA4ICADkbAwP+wj6MAAAAAElFTkSuQmCC";
    const JPEG: &str = "/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/2wBDAQkJCQwLDBgNDRgyIRwhMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjIyMjL/wAARCAADAAIDASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwD3+iiigD//2Q==";
    const WEBP: &str = "UklGRiQAAABXRUJQVlA4IBgAAAAwAQCdASoCAAMAAUAmJaQAA3AA/vz0AAA=";
    fn store() -> Store {
        let store = Store {
            conn: rusqlite::Connection::open_in_memory().unwrap(),
        };
        store.conn.execute_batch("CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        CREATE TABLE agent_sessions(id TEXT PRIMARY KEY,title TEXT,updated_at INTEGER,project_id TEXT,pinned INTEGER,owner_key TEXT,archived_at INTEGER DEFAULT 0,deleted_at INTEGER DEFAULT 0);
        CREATE TABLE agent_messages(id TEXT PRIMARY KEY,session_id TEXT,role TEXT,content TEXT,model_metadata TEXT,created_at INTEGER DEFAULT 0);
        CREATE TABLE agent_reply_versions(session_id TEXT,root_id TEXT,selected_id TEXT);
        CREATE TABLE agent_model_pending(session_id TEXT PRIMARY KEY,message_id TEXT);
        INSERT INTO agent_sessions(id,title,updated_at,project_id,pinned,owner_key) VALUES ('a','a',0,'general',0,'guest'),('b','b',0,'general',0,'guest'),('private','private',0,'general',0,'other');").unwrap();
        crate::agent_project_trash::initialize(&store).unwrap();
        initialize(&store).unwrap();
        crate::agent_project_trash::initialize(&store).unwrap();
        initialize(&store).unwrap();
        store
    }
    fn upload_text(store: &Store, session: &str, name: &str, text: &str) -> Value {
        upload(store,&json!({"sessionId":session,"name":name,"mimeType":"application/json","data":STANDARD.encode(text)})).unwrap()
    }
    fn add_message(store: &Store, id: &str, session: &str) {
        store.conn.execute("INSERT INTO agent_messages(id,session_id,role,content,model_metadata) VALUES (?1,?2,'user','','{}')",params![id,session]).unwrap();
    }
    #[test]
    fn legacy_metadata_is_backfilled_once_and_remains_immutable() {
        let store = store();
        store.conn.execute_batch("DROP TRIGGER agent_attachments_immutable;DROP TABLE agent_attachments;
        CREATE TABLE agent_attachments(id TEXT PRIMARY KEY,owner_key TEXT NOT NULL,session_id TEXT NOT NULL,name TEXT NOT NULL,mime_type TEXT NOT NULL,kind TEXT NOT NULL,data BLOB NOT NULL,sha256 TEXT NOT NULL,created_at INTEGER NOT NULL);").unwrap();
        let bytes = STANDARD.decode(PNG).unwrap();
        let canonical = validate("legacy.png", "image/png", &bytes).unwrap();
        store.conn.execute("INSERT INTO agent_attachments VALUES ('legacy','guest','a','legacy.png','image/png','image',?1,?2,0)",params![bytes,canonical["sha256"].as_str().unwrap()]).unwrap();
        crate::agent_project_trash::initialize(&store).unwrap();
        initialize(&store).unwrap();
        crate::agent_project_trash::initialize(&store).unwrap();
        initialize(&store).unwrap();
        let public = draft(&store, "a").unwrap();
        assert_eq!(public[0]["size"], canonical["size"]);
        assert_eq!(public[0]["width"], 2);
        assert_eq!(public[0]["height"], 3);
        assert!(
            store
                .conn
                .execute("UPDATE agent_attachments SET size=99 WHERE id='legacy'", [])
                .is_err()
        );
        assert!(
            store
                .conn
                .execute(
                    "UPDATE agent_attachments SET data=x'00' WHERE id='legacy'",
                    []
                )
                .is_err()
        );
    }
    #[test]
    fn metadata_poll_does_not_fetch_attachment_blobs() {
        let store = store();
        let args = json!({"sessionId":"a","name":"map.png","mimeType":"image/png","data":PNG});
        let first = upload(&store, &args).unwrap();
        let second = upload(&store, &args).unwrap();
        add_message(&store, "with-image", "a");
        bind(&store, "a", "with-image", &json!([first["id"]])).unwrap();
        // SQLite TEXT intentionally cannot be read as Vec<u8>. Public polling must still work
        // from immutable cached metadata, while explicit content reads fail integrity checks.
        store.conn.execute_batch("DROP TRIGGER agent_attachments_immutable;UPDATE agent_attachments SET data='must not be fetched';").unwrap();
        let drafts = draft(&store, "a").unwrap();
        assert_eq!(drafts, json!([second]));
        let mut messages = vec![json!({"id":"with-image","role":"user","content":""})];
        decorate_messages(&store, "a", &mut messages).unwrap();
        assert_eq!(messages[0]["attachments"], json!([first.clone()]));
        assert!(read_bytes(&store, "a", first["id"].as_str().unwrap()).is_err());
    }
    #[test]
    fn validates_types_content_names_and_bounded_headers() {
        let png = STANDARD.decode(PNG).unwrap();
        let webp = STANDARD.decode(WEBP).unwrap();
        let jpeg = STANDARD.decode(JPEG).unwrap();
        let lossless = STANDARD
            .decode("UklGRh4AAABXRUJQVlA4TBEAAAAvAYAAAAfQ//73v/+BiOh/AAA=")
            .unwrap();
        for (name, mime, bytes) in [
            ("map.png", "image/png", &png),
            ("map.webp", "image/webp", &webp),
            ("map.jpg", "image/jpeg", &jpeg),
            ("map-lossless.webp", "image/webp", &lossless),
        ] {
            let metadata = validate(name, mime, bytes).unwrap();
            assert_eq!(metadata["width"], 2);
            assert_eq!(metadata["height"], 3);
            assert_eq!(metadata["kind"], "image");
            assert_eq!(metadata["sha256"].as_str().unwrap().len(), 64);
            assert!(
                validate(
                    name,
                    if mime == "image/jpeg" {
                        "image/png"
                    } else {
                        "image/jpeg"
                    },
                    bytes
                )
                .is_err()
            );
            assert!(validate(name, mime, &bytes[..bytes.len() - 1]).is_err());
        }
        let mut corrupt = png.clone();
        corrupt[29] ^= 1;
        assert!(validate("map.png", "image/png", &corrupt).is_err());
        for (w, h) in [(8193u32, 1u32), (6001, 4000), (0, 1)] {
            let mut huge = png.clone();
            huge[16..20].copy_from_slice(&w.to_be_bytes());
            huge[20..24].copy_from_slice(&h.to_be_bytes());
            let crc = crc32(&huge[12..29]);
            huge[29..33].copy_from_slice(&crc.to_be_bytes());
            assert!(validate("large.png", "image/png", &huge).is_err());
        }
        for name in [
            "",
            "../file.txt",
            "folder/file.txt",
            "folder\\file.txt",
            ".",
            "..",
            "a\n.txt",
            " a.txt",
        ] {
            assert!(validate(name, "text/plain", b"ok").is_err(), "{name:?}");
        }
        assert!(validate(&"a".repeat(129), "text/plain", b"ok").is_err());
        assert!(validate("file.txt", "text/plain", &vec![b'a'; MAX_TEXT + 1]).is_err());
        assert!(validate("file.txt", "text/plain", b"a\0b").is_err());
        assert!(validate("file.txt", "text/plain", b"a\x08b").is_err());
        assert!(validate("file.txt", "text/plain", b"\xff").is_err());
        assert!(validate("fake.png", "image/png", b"hello").is_err());
        assert!(validate("x.svg", "image/svg+xml", b"<svg/>").is_err());
        let text = validate("script.html", "text/html", b"<script>alert(1)</script>\n").unwrap();
        assert_eq!(text["mimeType"], "text/plain");
        assert_eq!(text["kind"], "text");
    }
    #[test]
    fn owner_session_lifecycle_limits_and_explicit_read() {
        let store = store();
        let attachment = upload_text(&store, "a", "notes.txt", "中文\nnotes");
        let id = attachment["id"].as_str().unwrap();
        assert!(read_bytes(&store, "b", id).is_err());
        assert!(read_bytes(&store, "private", id).is_err());
        assert!(
            upload(
                &store,
                &json!({"sessionId":"private","name":"x","data":"YQ=="})
            )
            .is_err()
        );
        assert!(delete(&store, &json!({"sessionId":"b","id":id})).is_err());
        for index in 0..3 {
            upload_text(&store, "a", &format!("{index}.txt"), "x");
        }
        assert!(
            upload(
                &store,
                &json!({"sessionId":"a","name":"overflow.txt","data":"YQ=="})
            )
            .is_err()
        );
        assert_eq!(draft(&store, "a").unwrap().as_array().unwrap().len(), 4);
        store
            .conn
            .execute("UPDATE agent_sessions SET archived_at=1 WHERE id='a'", [])
            .unwrap();
        assert_eq!(
            content(&store, &json!({"sessionId":"a","id":id})).unwrap()["data"],
            STANDARD.encode("中文\nnotes")
        );
        assert!(delete(&store, &json!({"sessionId":"a","id":id})).is_err());
        assert!(upload(&store, &json!({"sessionId":"a","name":"x","data":"YQ=="})).is_err());
        store
            .conn
            .execute("UPDATE agent_sessions SET archived_at=0 WHERE id='a'", [])
            .unwrap();
        delete(&store, &json!({"sessionId":"a","id":id})).unwrap();
        assert!(read_bytes(&store, "a", id).is_err());
        store
            .conn
            .execute("INSERT INTO agent_model_pending VALUES ('b','reply')", [])
            .unwrap();
        assert!(upload(&store, &json!({"sessionId":"b","name":"x","data":"YQ=="})).is_err());
    }
    #[test]
    fn draft_binding_is_atomic_and_edit_links_are_shared_immutable() {
        let store = store();
        let first = upload_text(&store, "a", "first.txt", "first");
        let second = upload_text(&store, "a", "second.txt", "second");
        let outside = upload_text(&store, "b", "outside.txt", "private");
        add_message(&store, "original", "a");
        add_message(&store, "other-session", "b");
        assert!(bind(&store, "a", "other-session", &json!([first["id"]])).is_err());
        assert!(bind(&store, "a", "missing", &json!([first["id"]])).is_err());
        assert!(bind(&store, "a", "original", &json!([first["id"], first["id"]])).is_err());
        assert!(
            bind(
                &store,
                "a",
                "original",
                &json!([first["id"], outside["id"]])
            )
            .is_err()
        );
        assert_eq!(message_ids(&store, "a", "original").unwrap(), json!([]));
        {
            let _tx = store.conn.unchecked_transaction().unwrap();
            bind(&store, "a", "original", &json!([first["id"], second["id"]])).unwrap();
            assert_eq!(draft(&store, "a").unwrap(), json!([]));
            // Dropping the caller's transaction models a later message validation failure.
        }
        assert_eq!(draft(&store, "a").unwrap().as_array().unwrap().len(), 2);
        let tx = store.conn.unchecked_transaction().unwrap();
        bind(&store, "a", "original", &json!([second["id"], first["id"]])).unwrap();
        tx.commit().unwrap();
        add_message(&store, "edited", "a");
        clone_links(&store, "a", "original", "edited").unwrap();
        assert_eq!(
            message_ids(&store, "a", "edited").unwrap(),
            json!([second["id"], first["id"]])
        );
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT count(*) FROM agent_attachments WHERE session_id='a'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            2
        );
        assert!(bind(&store, "a", "edited", &json!([first["id"]])).is_err());
        assert!(delete(&store, &json!({"sessionId":"a","id":first["id"]})).is_err());
        assert!(
            store
                .conn
                .execute(
                    "UPDATE agent_attachments SET data=x'00' WHERE id=?1",
                    [first["id"].as_str().unwrap()]
                )
                .is_err()
        );
        let mut messages = vec![json!({"id":"edited","role":"user","content":""})];
        decorate_messages(&store, "a", &mut messages).unwrap();
        assert!(messages[0]["attachments"][0].get("data").is_none());
        assert!(messages[0]["attachments"][0].get("text").is_none());
        let context = context_message(&store, "a", &messages[0]).unwrap();
        assert_eq!(context["content"], "");
        assert_eq!(context["keeperAttachments"][0]["text"], "second");
        assert!(context.to_string().find("base64").is_none());
    }
    #[test]
    fn attachment_only_messages_survive_context_selection() {
        let store = store();
        let attachment = upload_text(&store, "a", "instructions.txt", "用户选择的文本");
        add_message(&store, "question", "a");
        bind(&store, "a", "question", &json!([attachment["id"]])).unwrap();
        add_message(&store, "boundary", "a");
        store
            .conn
            .execute(
                "UPDATE agent_messages SET role='assistant' WHERE id='boundary'",
                [],
            )
            .unwrap();
        let context = crate::agent::context_messages(&store, "a", "boundary").unwrap();
        assert_eq!(context.len(), 1);
        assert_eq!(context[0]["content"], "");
        assert_eq!(context[0]["keeperAttachments"][0]["text"], "用户选择的文本");
    }
    #[test]
    fn image_capability_and_integrity_are_checked_before_use() {
        let store = store();
        let image = upload(
            &store,
            &json!({"sessionId":"a","name":"map.png","mimeType":"image/png","data":PNG}),
        )
        .unwrap();
        let linked = json!([image["id"]]);
        assert!(
            ensure_supported(
                &store,
                "a",
                &linked,
                &json!({"enabled":true,"hasApiKey":true,"supportsImages":false})
            )
            .is_err()
        );
        assert!(
            ensure_supported(
                &store,
                "a",
                &linked,
                &json!({"enabled":true,"hasApiKey":true,"supportsImages":true})
            )
            .is_ok()
        );
        assert!(ensure_supported(&store, "a", &linked, &json!({"enabled":false})).is_ok());
        add_message(&store, "image-message", "a");
        bind(&store, "a", "image-message", &linked).unwrap();
        let context = context_message(
            &store,
            "a",
            &json!({"id":"image-message","role":"user","content":""}),
        )
        .unwrap();
        assert!(context["keeperAttachments"][0].get("text").is_none());
        assert!(context["keeperAttachments"][0].get("data").is_none());
        store.conn.execute_batch("DROP TRIGGER agent_attachments_immutable;UPDATE agent_attachments SET sha256='wrong';").unwrap();
        assert!(read_bytes(&store, "a", image["id"].as_str().unwrap()).is_err());
    }
}
