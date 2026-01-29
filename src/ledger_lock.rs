use std::sync::OnceLock;
use tokio::sync::Mutex;

/// Global mutex to serialize all Ledger/HID access.
///
/// On Windows in particular, concurrent Ledger operations can trigger HIDAPI
/// errors like "Overlapped I/O operation is in progress" and can also appear
/// to "freeze" callers if the async runtime thread is blocked.
static LEDGER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn ledger_lock() -> &'static Mutex<()> {
    LEDGER_LOCK.get_or_init(|| Mutex::new(()))
}


