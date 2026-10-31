//! Explicit, bounded reads for the sensitive inspector. Status never uses this.
use rustclipsync_core::state::InboxItem;
use serde_json::{json, Value};
use std::io;

pub fn read_chunk(item: &InboxItem, id: &str, offset: usize) -> io::Result<Value> {
    if item.id != id || offset > item.text.len() || !item.text.is_char_boundary(offset) {
        return Err(crate::config::invalid("stale inbox or invalid offset"));
    }
    // Even six-byte JSON escapes fit comfortably inside the 256 KiB IPC frame.
    let mut end = (offset + 16 * 1024).min(item.text.len());
    while !item.text.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"id":item.id,"receivedAt":item.received_at,"offset":offset,
        "nextOffset":end,"total":item.text.len(),"text":&item.text[offset..end]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_inbox_reads_are_bounded_unicode_safe_and_reject_stale_items() {
        let item = InboxItem {
            id: rustclipsync_core::state::random_uuid().unwrap(),
            text: format!("{}😀中文{}", "\0\"\\".repeat(6000), "😀".repeat(5000)),
            received_at: 42,
        };
        let mut offset = 0;
        let mut restored = String::new();
        while offset < item.text.len() {
            let chunk = read_chunk(&item, &item.id, offset).unwrap();
            assert!(serde_json::to_vec(&chunk).unwrap().len() < crate::ipc::MAX_FRAME);
            assert_eq!(chunk["offset"], offset);
            restored.push_str(chunk["text"].as_str().unwrap());
            let next = chunk["nextOffset"].as_u64().unwrap() as usize;
            assert!(next > offset);
            offset = next;
        }
        assert_eq!(restored, item.text);
        assert!(read_chunk(&item, "old item", 0).is_err());
        assert!(read_chunk(&item, &item.id, item.text.len() + 1).is_err());
        assert!(read_chunk(&item, &item.id, 18001).is_err());
        assert_eq!(read_chunk(&item, &item.id, offset).unwrap()["text"], "");
    }
}
