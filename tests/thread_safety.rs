//! Regression test for the `unsendable` panic.
//!
//! `EiType` must be `Send + Sync` so it can be created on one thread and used from
//! another (for example, a worker in a `ThreadPoolExecutor`).
//!
//! Before the fix, `EiType` was `#[pyclass(unsendable)]`; PyO3 then panicked with
//! "EiType is unsendable, but sent to another thread" whenever a cached typer was
//! reused from a different worker thread than the one that created it. Because the
//! panic surfaced in Python as `pyo3_runtime.PanicException` (a `BaseException`), it
//! slipped past callers' `except Exception` handlers and silently dropped the text.

use eitype::EiType;

// Compile-time guarantee from a downstream crate's perspective — this is exactly
// what the Python bindings and any other consumer rely on. If a future change makes
// `EiType` `!Send`/`!Sync` again, this fails to build.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<EiType>();
};

/// Move a connected typer to another thread and type from it — the precise pattern
/// that used to panic. Requires a Wayland desktop with EI support, so it is gated
/// behind the same feature as the other integration tests.
#[cfg(feature = "wayland-integration-tests")]
#[test]
fn test_type_from_a_different_thread() {
    use eitype::EiTypeConfig;

    let typer =
        EiType::connect_portal(EiTypeConfig::default()).expect("Failed to connect to portal");

    // Created on this thread, moved into and used from another. The `move` closure
    // compiles only because `EiType: Send`, and the call itself would have tripped
    // the `unsendable` thread-affinity assertion before the fix.
    let handle = std::thread::spawn(move || {
        typer
            .type_text("Typed from a spawned thread - no unsendable panic.")
            .expect("typing from another thread failed");
    });

    handle.join().expect("typing thread panicked");
}
