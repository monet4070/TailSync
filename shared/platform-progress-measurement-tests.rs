/// Injects contention at the real platform progress store. This is a diagnostic
/// experiment, not a device-throughput or 20ms budget acceptance gate.
#[test]
#[ignore = "200 real callback samples with injected lock contention"]
fn platform_progress_callback_contention_measurement() {
    let mut samples = Vec::new();
    for index in 0..200 {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guard = super::FILE_PROGRESS.lock().unwrap();
            ready_tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(50));
        });
        ready_rx.recv().unwrap();
        let start = std::time::Instant::now();
        set_file_batch_progress(progress("lock-measurement", "synthetic-peer", index));
        samples.push(start.elapsed().as_micros());
        holder.join().unwrap();
    }
    samples.sort_unstable();
    println!(
        "{}",
        serde_json::json!({"measurement":"real_platform_progress_callback", "samples":samples.len(),
        "injected_lock_hold_ms":50, "callback_us":{"p50":samples[99],"p99":samples[197],"max":samples[199]}})
    );
    clear_file_progress_scope(Some("lock-measurement"), Some("synthetic-peer"));
    assert!(
        samples[197] >= 20_000,
        "fault injection did not exercise the contested progress lock"
    );
}
