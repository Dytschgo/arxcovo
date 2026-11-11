# Windows SCM source mutation evidence

Sol / high; local Windows fake and synthetic tests only. Each mutation was applied separately to actual implementation source, compiled, and tested with `cargo nextest run -p arxd service::owner_scm --locked --no-fail-fast`. Originals restored after every run. No native SCM constructor or SCM call was executed.

- `create-auto`: failed `service::owner_scm_windows::tests::constant_mirrors`.
- `start-without-auto`: failed `service::owner_scm::tests::boot_and_crash_prefix_model`, `service::owner_scm::tests::create_races_and_start_already_running`, `service::owner_scm::tests::transition_matrix_exact_calls`.
- `stop-demand-before-stop`: failed `service::owner_scm::tests::boot_and_crash_prefix_model`, `service::owner_scm::tests::deadlines_and_one_successful_control`, `service::owner_scm::tests::failure_after_stop_keeps_auto_until_rerun`, `service::owner_scm::tests::transition_matrix_exact_calls`.
- `disabled-is-ours`: failed `service::owner_scm::tests::classification_matrix`.
- `1077-is-failed`: failed `service::owner_scm::tests::status_all_states_and_exit_pairs`.
- `two-failure-actions`: failed `service::owner_scm::tests::installed_exact_set_and_failure_policy`.
- `case-insensitive-image`: failed `service::owner_scm::tests::classification_matrix`.
- `ignore-delete-error`: failed `service::owner_scm::tests::per_call_faults_halt_without_proof`.
- `remove-deadline`: failed `service::owner_scm::tests::boot_and_crash_prefix_model`, `service::owner_scm::tests::deadlines_and_one_successful_control`.
- `register-allows-start-pending`: failed `service::owner_scm::tests::transition_matrix_exact_calls`.
