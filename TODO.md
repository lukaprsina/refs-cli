# TODO

## Tests that were never seen failing

These were written after the code they cover (PR #32), so they may pass for the wrong reason. For each: break the behaviour on purpose (or `git stash` the implementation), confirm the test goes red, restore.

`tests/plan_checkouts.rs`
- [ ] `every_active_repo_and_each_other_listed_name_is_inspected_once`
- [ ] `inspect_failures_are_all_collected`
- [ ] `a_failed_action_on_a_listed_checkout_holds_back_the_writes`
- [ ] `outcomes_follow_from_the_plan`
- [ ] `a_withheld_repo_keeps_its_checkout_and_is_not_in_the_block`

`tests/plan_lock.rs`
- [ ] `the_lock_is_written_only_when_it_differs_and_takes_the_pin_as_resolved`

`tests/cli.rs`
- [ ] `a_disable_with_a_broken_repo_still_removes_its_checkout_and_updates_the_block`

`tests/cli_output.rs` (all but the `init` tests were green on first run)
- [ ] `an_in_sync_sync_says_so_in_one_line_on_stderr`
- [ ] `a_first_sync_names_the_lock_each_checkout_and_each_agent_file`
- [ ] `lock_says_it_updated_the_lock_and_then_that_nothing_changed`
- [ ] `quiet_hides_status_lines_but_not_problems`
- [ ] `a_failed_sync_ends_with_the_hint_to_run_sync_and_a_good_one_has_none`
- [ ] `add_says_what_it_changed_and_hints_only_with_no_sync`
- [ ] `remove_and_disable_say_which_checkout_went`
- [ ] `a_changed_ref_moves_the_checkout_and_says_so`
- [ ] `an_edit_that_changes_nothing_says_so`
- [ ] `an_edit_whose_sync_fails_keeps_refs_toml_and_hints_to_sync`
- [ ] `check_keeps_exit_3_for_out_of_date_and_1_for_refusals_and_prints_no_hint`
- [ ] `list_is_data_on_stdout_and_nothing_on_stderr`
- [ ] `the_binary_sync_with_nothing_to_fetch_says_what_it_changed_then_that_nothing_did`
- [ ] `the_binary_edits_without_sync_say_what_changed_and_hint`

## Known gaps

- [ ] `list` dimming has no test through the binary (a pipe is never a terminal); only `disabled_lines_are_dimmed_only_with_color` covers it.
- [ ] Real terminal detection in `cli::run` is untested; only the `Terminal` seam is.
- [ ] If every active Repo fails to lock after a `remove`/`disable`, the Managed block is left as is rather than stripped.
- [ ] #33: report a replaced Checkout distinctly from a moved one.
