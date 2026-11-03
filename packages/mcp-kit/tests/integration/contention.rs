#[path = "fixture/contention.rs"]
mod workload;

#[test]
fn contention_bounds_producers_and_preserves_keys_cancel_and_fanout_after_slow_output() {
    for streams in [1, 16, 64] {
        eprintln!("{}", workload::run(streams));
    }
}
