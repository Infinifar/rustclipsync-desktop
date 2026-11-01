use rustclipsync_agent::{
    config::HistorySettings,
    history::{self, Entry, HistoryData},
    local_history::LocalHistory,
    platform::storage,
};
use rustclipsync_core::adapters::{Direction, HistorySink};

fn fixture(count: usize, length: usize) -> (tempfile::TempDir, LocalHistory, [u8; 32]) {
    let temp = tempfile::tempdir().unwrap();
    let key = [7; 32];
    let data = HistoryData {
        entries: (0..count)
            .map(|index| Entry {
                id: format!("00000000-0000-4000-8000-{index:012}"),
                text: format!("{index:04} {}", "a".repeat(length.saturating_sub(5))),
                direction: "upload".into(),
                timestamp: 1_000_000 - index as u64,
                favorite: index == 0,
            })
            .collect(),
    };
    storage::atomic_write(
        &temp.path().join("history"),
        &history::seal(&key, &data).unwrap(),
    )
    .unwrap();
    let h = LocalHistory::open(
        temp.path().join("history"),
        key,
        HistorySettings {
            enabled: true,
            limit: 50,
            days: 7,
        },
    );
    (temp, h, key)
}
#[test]
fn pagination_conditions_expiry_and_restart_have_coherent_versions() {
    let (temp, h, key) = fixture(25, 512);
    let first = h.page(0, "", false, None, None, 1_000_000).unwrap();
    assert_eq!(first["items"].as_array().unwrap().len(), 20);
    assert_eq!(first["total"], 25);
    let epoch = first["historyEpoch"].as_str().unwrap();
    let version = first["historyRevision"].as_u64().unwrap();
    assert!(h
        .page(0, "", false, Some(epoch), Some(version), 1_000_000)
        .unwrap()["unchanged"]
        .as_bool()
        .unwrap());
    assert_eq!(
        h.page(20, "", false, None, None, 1_000_000).unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert_eq!(
        h.page(0, "0000", true, None, None, 1_000_000).unwrap()["total"],
        1
    );
    let persisted = std::fs::read(temp.path().join("history")).unwrap();
    h.prune(1_000_000).unwrap();
    assert_eq!(h.revision(), version);
    assert_eq!(
        std::fs::read(temp.path().join("history")).unwrap(),
        persisted
    );
    let restarted = LocalHistory::open(
        temp.path().join("history"),
        key,
        HistorySettings {
            enabled: true,
            limit: 50,
            days: 7,
        },
    );
    assert!(!restarted
        .page(0, "", false, Some(epoch), Some(version), 1_000_000)
        .unwrap()["unchanged"]
        .as_bool()
        .unwrap());
    h.configure(HistorySettings {
        enabled: true,
        limit: 50,
        days: 1,
    });
    let expired = h
        .page(20, "", false, Some(epoch), Some(version), 1_100_000)
        .unwrap();
    assert_eq!(expired["total"], 1); // Favorite survives TTL.
    assert_eq!(expired["offset"], 0);
    assert_ne!(expired["historyRevision"], version);
    assert!(h.text("missing").unwrap().is_none());
    let id = first["items"][0]["id"].as_str().unwrap();
    assert!(h.text(id).unwrap().unwrap().starts_with("0000"));
}
#[test]
fn disabled_recording_and_unchanged_configuration_preserve_revision() {
    let (_temp, h, _) = fixture(2, 100);
    let cfg = HistorySettings {
        enabled: true,
        limit: 50,
        days: 7,
    };
    let revision = h.revision();
    h.configure(cfg);
    assert_eq!(h.revision(), revision);
    h.configure(HistorySettings {
        enabled: false,
        limit: 50,
        days: 7,
    });
    let revision = h.revision();
    h.record("not saved", Direction::Upload, 1_000_001).unwrap();
    assert_eq!(h.revision(), revision);
    assert_eq!(
        h.page(0, "", false, None, None, 1_000_001).unwrap()["total"],
        2
    );
}

// Thread-local allocation accounting avoids counting test harness / other test
// threads. This measures allocation traffic, not process resident memory.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! { static ALLOCATIONS: Cell<(bool,usize)> = const { Cell::new((false,0)) }; }
struct Counted;
fn account(bytes: usize) {
    let _ = ALLOCATIONS.try_with(|c| {
        let (active, total) = c.get();
        if active {
            c.set((active, total + bytes));
        }
    });
}
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        account(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        account(l.size());
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        account(n);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
fn allocated(action: impl FnOnce()) -> usize {
    ALLOCATIONS.with(|v| v.set((true, 0)));
    action();
    ALLOCATIONS.with(|v| {
        let (_, bytes) = v.get();
        v.set((false, 0));
        bytes
    })
}
#[test]
#[ignore = "Explicit allocation comparison on synthetic 16 MiB history"]
fn large_history_reads_do_not_clone_the_body_for_pages_or_no_op_pruning() {
    let (_temp, h, _) = fixture(16, 1024 * 1024);
    let snapshot = allocated(|| {
        std::hint::black_box(h.data().unwrap());
    });
    let page = allocated(|| {
        std::hint::black_box(h.page(0, "", false, None, None, 1_000_000).unwrap());
    });
    let epoch = h.epoch();
    let revision = h.revision();
    let conditional = allocated(|| {
        for _ in 0..100 {
            assert_eq!(
                h.page(0, "", false, Some(&epoch), Some(revision), 1_000_000)
                    .unwrap()["unchanged"],
                true
            );
        }
    });
    let cleanup = allocated(|| {
        for _ in 0..100 {
            h.prune(1_000_000).unwrap();
        }
    });
    assert!(snapshot >= 16 * 1024 * 1024);
    assert!(page < 128 * 1024, "page allocated {page} bytes");
    assert!(
        conditional < 256 * 1024,
        "conditional reads allocated {conditional} bytes"
    );
    assert_eq!(cleanup, 0);
    println!("history_allocation_bytes: full_snapshot={snapshot}, page={page}, conditional_100={conditional}, no_op_prune_100={cleanup}");
}
