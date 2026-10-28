# Verification and ownership map

## Verification scope

The single integration binary is `tests/integration/main.rs`. Test names below
are qualified by their module. Process fixtures re-execute that binary and use
temporary Acme roots. Core tests use explicit timestamps, never timer sleeps.
This map records coverage, not a log of executed checks.

Package checks are selected independently through the targets in
[development](../docs/development.md). Focused iteration uses `make test-focus`
with a test-name filter and `TEST_TARGET='--test integration'`. Tests requiring
sockets, processes or signals need host access when sandboxed. Test coverage
does not replace acceptance on a different platform or in a consuming product.

## Existing coverage

| Requirement | Test or acceptance boundary | Owner |
| --- | --- | --- |
| Private directories and sockets; actual same-user peers | `transport::real_socket_checks_peer_and_forged_other_uid_is_refused` | Library |
| Other UID refused before frame reading | Same test injects a forged UID into `check_uid`; real second-user connection is unverified | Library, platform gap |
| Permissive or symlink root refused | `transport::private_roots_refuse_symlinks_and_permissive_directories`; foreign-owner rejection has no real second-user fixture | Library |
| Account-home root ignores changed HOME | `processes::account_home_ignores_home_environment_in_a_separate_process` | Library |
| Overlong socket path refused | `transport::absent_inspection_creates_nothing_and_long_paths_are_refused` | Library |
| Racing helpers converge; typed losers; every client reaches winner | `processes::simultaneous_starters_converge_and_losers_exit_distinctly`, `processes::clients_connect_or_start_race_reaches_one_winner`; fixture maps `AlreadyRunning` to distinct exit 73 | Library |
| Starting owner waits for exiting holder | `processes::starting_owner_waits_for_exiting_lease_holder` | Library |
| One readiness wait activates once while its child has not acquired the lease | `processes::one_client_activates_once_while_its_child_has_not_acquired_the_lease` holds a real child before acquisition, checks bounded expiry and counts activations, then releases and reaps it | Library |
| Stale socket replaced only with lease; live socket preserved; duplicate bind refused | `transport::stale_socket_replacement_and_live_lease_preservation` | Library |
| SIGKILL releases lease; successor replaces stale endpoint | `processes::sigkill_releases_kernel_lease_and_next_client_replaces_stale_socket` | Library |
| Detached manager survives parent exit, SIGTERM and SIGHUP to parent group | `processes::detached_manager_survives_parent_exit_and_parent_group_signals` | Library |
| Minimal allowlist, null stdin, new session and group | `processes::detached_start_has_only_allowlist_null_stdin_and_new_session` | Library |
| Non-launching absent inspection creates nothing | `transport::absent_inspection_creates_nothing_and_long_paths_are_refused` | Library |
| Reserved controls answer under ordinary saturation | `transport::saturated_work_lane_keeps_controls_responsive_and_refuses_capacity` | Library |
| Busy retirement returns product blockers; empty retirement commits | Same saturation test, `upgrades::newer_client_retires_empty_older_manager_and_reuses_busy_compatible_one` | Library mechanics; product blocker contents |
| Atomic retirement fences admission and preserves earlier work | `lifecycle::retirement_fences_admission_and_preserves_accepted_work`, `lifecycle::admission_during_policy_callback_cancels_retirement` | Library |
| Drain refuses new work and finishes admitted replies and holds | `lifecycle::drain_finishes_admitted_work_and_transferred_holds`, saturation test | Library |
| Continuing controls cannot extend settled shutdown; admitted replies finish | `core::tests::control_admission_closes_when_draining_work_and_holds_settle`, `core::tests::retirement_and_idle_exit_close_controls_but_preserve_existing_replies`, `transport::drain_releases_lease_while_silent_control_connections_continue` | Library |
| Owned endpoints removed before lease release | Saturation test; `transport::stale_socket_replacement_and_live_lease_preservation` observes lease still held after unlink | Library |
| Cleanup never removes a replacement socket | `transport::cleanup_preserves_a_replaced_socket_identity` | Library |
| Grace, immediate-empty and explicit policies; held state prevents exit | `lifecycle::three_policies_use_a_controllable_clock_and_observers_never_renew_grace` | Library interface; product policy and hold placement |
| Observers finish without extending grace or vetoing retirement | Same policy and retirement-fencing tests; continuing arrivals covered by the drain test above | Library |
| Unknown controls/versions refuse without effects; capability refusal precedes dispatch | `transport::controls_ignore_product_version_and_unknown_actions_have_no_effects` | Library |
| Explicit immediate upgrade retires idle manager, preserves busy compatible and incompatible controls | `upgrades::newer_client_retires_empty_older_manager_and_reuses_busy_compatible_one` | Library; caller selects this manual retirement attempt |
| Semantic release order ignores build metadata and refuses invalid labels | `version::tests::release_precedence_does_not_use_string_or_build_metadata_order`, `version::tests::unknown_and_unbounded_labels_are_not_ordered` | Library |
| Versioned connection replaces an empty older process and bounds stale-launcher retries | `upgrades::versioned_connection_replaces_an_empty_older_process`, `upgrades::versioned_connection_refuses_a_stale_launcher_without_restart_churn` | Library |
| Concurrent newer clients converge on one replacement | `upgrades::concurrent_versioned_clients_converge_on_one_replacement` | Library |
| Lost retirement result safely reobserves identity; pre-feature and unknown builds stay conservative | `upgrades::lost_version_retirement_result_reobserves_before_starting_a_replacement`, `upgrades::pre_feature_busy_manager_uses_the_existing_empty_retirement_control`, `upgrades::an_unorderable_running_build_is_not_retired` | Library |
| Socket publication cannot cause an unsafe connection or an unready-permission failure while leased | `transport::owned_socket_permissions_in_progress_require_a_live_lease_and_never_connect` | Library |
| Busy version replacement preserves work and retires after holds settle | `upgrades::newer_client_schedules_busy_retirement_without_interrupting_work`, `core::tests::pending_retirement_waits_for_work_and_bypasses_the_idle_policy` | Library; product hold placement |
| Equal, older, metadata-only, invalid or stale-incarnation notifications cannot retire a manager | `upgrades::equal_older_and_metadata_only_versions_do_not_retire_a_manager`, `upgrades::manager_validates_version_and_incarnation_before_scheduling_retirement`, `upgrades::invalid_client_release_starts_nothing` | Library |
| Stale incarnation cannot stop replacement | `upgrades::stale_incarnation_cannot_retire_or_drain_replacement` | Library |
| Oversized frames refused, never truncated; partial bodies fail | `transport::frames_refuse_oversize_and_partial_frames_without_truncation`, `transport::real_oversize_and_idle_read_deadline_release_connection_slots` | Library |
| Connection capacity explicitly refused; idle read releases slot | Saturation and idle-read tests above | Library |
| One-shot lost admission acknowledgement is ambiguous, never replayed; ID preserved | `transport::lost_admission_ack_is_ambiguous_and_never_replayed` | Library transport; product durable reconciliation |
| Recovery replays identical ID/content only after product authorization, for lost acknowledgement and final result | `recovery::recovery_replays_the_identical_request_only_after_product_authorization` | Library orchestration; product replay authority |
| Durable result recovery and unresolved outcomes do not dispatch on successor | `recovery::recovery_can_return_a_durable_result_without_replaying_or_refuse_unsafe_replay` | Library orchestration; product durable outcome contents |
| Real manager death after recording an effect recovers without duplicate dispatch | `recovery::recovery_after_real_manager_death_reads_the_durable_outcome_without_duplicate_dispatch` | Library orchestration; product journal correctness |
| Connect-or-start waits through drain and reaches fresh process; bounded wait preserves held obligations | `recovery::connect_or_start_waits_for_drain_then_reaches_a_fresh_process`, `recovery::drain_wait_is_bounded_and_never_forces_live_holds` | Library |
| Killed owner produces EOF without releasing a resource before cleanup | `recovery::killed_owner_is_detected_but_cleanup_keeps_the_hold_until_explicit_release` | Library detection; product channel integration and cleanup |
| Force-stop is opt-in, rejects stale targets, stops held draining process and permits successor after lease release | `recovery::force_stop_is_opt_in_incarnation_checked_and_releases_the_kernel_lease` | Library; product crash recovery and external-worker fencing |
| Trickled reply bytes cannot renew a client frame deadline | `transport::client_admission_frame_deadline_does_not_renew_on_partial_reads`, `transport::client_result_frame_deadline_does_not_renew_on_partial_reads` cover partial prefixes and bodies, preserved request ID and no replay | Library |
| Client deadlines preserve buffered complete replies after peer exit | `client::tests::frame_deadline_preserves_buffered_reply_after_peer_exit` | Library |
| Overall readiness bounds silent endpoints and the combined admission/identity exchange | `transport::readiness_deadline_bounds_a_silent_endpoint`, `transport::readiness_deadline_spans_admission_and_identity_frames` | Library |
| Payload-free bounded lifecycle evidence | `lifecycle::bounded_events_are_payload_free_and_consumed` | Library; product logging/persistence |
| Login-service activation can replace direct spawn | `processes::service_launch_path_is_selected_without_spawning_the_binary` checks the interface boundary only | Product registration; live activation gap |
| Durable mutation deduplication and ID/content conflict | Keep application tests using its journal or operation store | Product |
| Successor reloads durable state under its lease, validates stored format and re-establishes resources before readiness | Keep application update/recovery tests; generic process fixtures establish fresh identity and lease exclusion only | Product |
| Updated executable and registered service agree on the release being activated | Stale-launcher fixture establishes bounded refusal, not installer correctness | Product |
| Long-lived owner channel, EOF policy and client/resource identity | Keep application owner-channel tests; use `Context::stream` and a transferred hold | Product |
| Blocker pagination, exact recovery authority and resource cleanup | Keep application recovery and blocker tests | Product |
| Device awake timers, actor queues, UI or gateway behavior | Keep application-specific tests | Product |
| Linux execution | Implemented cfg paths; host acceptance unverified | Platform gap |
| Windows named pipe and LockFileEx | Typed unsupported platform boundary; adapters unimplemented | Platform gap |

No real login registrations or second-user accounts are altered by these tests.
Resource and protocol behavior owned by products cannot be proved by a generic
echo fixture. Integrations must retain those tests when replacing their manager
mechanics. No RSS, wakeup or latency performance promise is established here.

## Acceptance criteria

The topic-level criteria in [endpoints](endpoints.md#acceptance-criteria),
[startup](startup.md#acceptance-criteria), [lifetime](lifetime.md#acceptance-criteria)
and [protocol](protocol.md#bounds-and-acceptance), together with
[updates](updates.md#acceptance-and-evidence-boundary), own observable success and
failure behavior. In particular, acceptance must distinguish:

- non-admission refusal from an admitted request with an uncertain result;
- closed mutation admission from completed shutdown and released exclusion;
- a live resource from retained historical records;
- implemented adapters from real-host evidence.

## Implementation and coverage gaps

| Gap | Contract / available evidence | Missing verification |
| --- | --- | --- |
| Trickled request-write coverage | [Limits](limits.md#verification-gap); request writes use an absolute frame deadline capped by readiness where applicable | A real peer that stalls request consumption cannot renew the total frame write deadline. |
| Oversized product response | The handler fixture can return a large value, but no test requests it | Bounded final `frame_too_large` error with matching ID and incarnation. |
| Invalid options and callback failure paths | Validation and panic containment exist in `server` | Invalid-option boundaries, callback panic isolation and cleanup after driver failure. |
| Actual foreign-user boundary | Shared UID comparison and real same-user sockets only | Another user's connection and foreign-owned paths, without modifying unrelated resources. |
| Registered-service activation and platform adapters | [Platforms](platforms.md#acceptance-boundaries) | Linux host execution, real login-service activation and Windows adapters/acceptance. |

These rows identify missing evidence; they are not claims that the
implementation fails. Durable ID deduplication, owner-channel semantics,
blocker authority and product resource cleanup remain product acceptance.
