//! Public Perfetto SQL tables, fixed projections. Limit+1 makes loss observable.
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_BUFFER_KIB: u32 = 2048;
pub(crate) const MIN_BUFFER_KIB: u32 = 256;
pub(crate) const MAX_BUFFER_KIB: u32 = 3840;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FillPolicy {
    #[default]
    Discard,
    RingBuffer,
}

pub(super) fn config(ms: u64, app: &str, buffer_kib: u32, policy: FillPolicy) -> String {
    let policy = match policy {
        FillPolicy::Discard => "DISCARD",
        FillPolicy::RingBuffer => "RING_BUFFER",
    };
    let mut config = format!(
        "buffers {{ size_kb: {buffer_kib} fill_policy: {policy} }}\nduration_ms: {ms}\ndata_sources {{ config {{ name: \"linux.ftrace\" ftrace_config {{ ftrace_events: \"sched/sched_switch\" ftrace_events: \"sched/sched_waking\" ftrace_events: \"binder/binder_transaction\" ftrace_events: \"binder/binder_transaction_received\" ftrace_events: \"block/block_rq_issue\" ftrace_events: \"block/block_rq_complete\" atrace_categories: \"gfx\" atrace_categories: \"view\" atrace_categories: \"dalvik\" atrace_categories: \"am\" atrace_apps: \"{app}\" }} }} }}\ndata_sources {{ config {{ name: \"linux.process_stats\" process_stats_config {{ scan_all_processes_on_start: true }} }} }}\n"
    );
    config.push_str("data_sources { config { name: \"android.surfaceflinger.frametimeline\" } }\n");
    config
}
pub(super) const QUERIES: &[(&str, &str)] = &[
    (
        "processes",
        "SELECT upid,pid,name,start_ts,end_ts FROM process ORDER BY upid LIMIT 2049",
    ),
    (
        "threads",
        "SELECT utid,tid,upid,name FROM thread ORDER BY utid LIMIT 2049",
    ),
    (
        "scheduling",
        "SELECT id,ts,dur,utid,state,io_wait,blocked_function,waker_utid FROM thread_state ORDER BY ts,id LIMIT 4097",
    ),
    (
        "slices",
        "SELECT s.id,s.ts,s.dur,s.name,s.category,s.track_id,t.utid FROM slice s LEFT JOIN thread_track t ON s.track_id=t.id ORDER BY s.ts,s.id LIMIT 4097",
    ),
    (
        "loss",
        "SELECT name,idx,severity,value FROM stats WHERE value != 0 ORDER BY name,idx LIMIT 1025",
    ),
    (
        "actual_frames",
        "SELECT id,ts,dur,name,upid,layer_name,present_type,jank_type FROM actual_frame_timeline_slice ORDER BY ts,id LIMIT 4097",
    ),
    (
        "expected_frames",
        "SELECT id,ts,dur,name,upid,layer_name FROM expected_frame_timeline_slice ORDER BY ts,id LIMIT 4097",
    ),
    ("bounds", "SELECT start_ts,end_ts FROM trace_bounds"),
    (
        "cpu_samples",
        "SELECT ts,utid,callsite_id FROM perf_sample ORDER BY ts LIMIT 4097",
    ),
    (
        "callsites",
        "SELECT id,parent_id,frame_id,depth FROM stack_profile_callsite ORDER BY id LIMIT 4097",
    ),
    (
        "frames",
        "SELECT f.id,f.name,f.rel_pc,f.mapping,m.build_id,m.name AS mapping_name FROM stack_profile_frame f LEFT JOIN stack_profile_mapping m ON f.mapping=m.id ORDER BY f.id LIMIT 4097",
    ),
];
