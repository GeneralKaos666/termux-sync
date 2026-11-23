from termux_sync.tui.models import apply_event, new_operation


def test_new_backup_operation_has_weighted_stages():
    state = new_operation("backup")
    assert [s.id for s in state.stages][0] == "preflight"
    assert sum(s.weight for s in state.stages) == 100
    assert state.overall_percent() == 0.0


def test_completed_stage_adds_its_weight():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "preflight", "label": "Preflight", "state": "complete", "index": 1, "total": 7})
    assert state.overall_percent() == 2.0


def test_progress_inside_stage_scales_weight():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "archive:home", "label": "Home", "state": "start", "index": 3, "total": 7})
    apply_event(state, {"event": "progress", "stage": "archive:home", "current": 50, "total": 100})
    assert state.overall_percent() == 25.0


def test_skipped_stage_normalises_remaining_weights():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "preflight", "label": "Preflight", "state": "skipped", "index": 1, "total": 7})
    assert [s.id for s in state.stages if s.state == "skipped"] == []
    assert sum(s.weight for s in state.stages) == 100


def test_unknown_stage_is_ignored():
    state = new_operation("backup")
    apply_event(state, {"event": "stage", "id": "future:thing", "label": "?", "state": "start", "index": 1, "total": 1})
    assert all(s.id != "future:thing" for s in state.stages)


def test_result_and_error_set_terminal_status():
    ok = new_operation("backup")
    apply_event(ok, {"event": "result", "ok": True, "name": "b1", "size": 10, "verified": True})
    assert ok.status == "succeeded" and ok.result["name"] == "b1"
    bad = new_operation("backup")
    apply_event(bad, {"event": "error", "message": "boom", "hint": "fix", "exit_code": 1})
    assert bad.status == "failed" and bad.failure["message"] == "boom"
    stopped = new_operation("backup")
    apply_event(stopped, {"event": "cancelled", "reason": "user"})
    assert stopped.status == "cancelled"


def test_restore_stages_use_restore_weights():
    state = new_operation("restore")
    assert [s.id for s in state.stages][0] == "plan"
    assert sum(s.weight for s in state.stages) == 100
