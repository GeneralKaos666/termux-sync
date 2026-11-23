import sys
import threading
import time
from pathlib import Path

from termux_sync.tui.runner import OperationRunner

FAKE = Path(__file__).parent / "fixtures" / "fake_engine.py"


def collect(mode, timeout=10):
    events, exits = [], []
    done = threading.Event()
    runner = OperationRunner(
        [sys.executable, str(FAKE)],
        on_event=events.append,
        on_exit=lambda code: (exits.append(code), done.set()),
    )
    runner.start("backup", mode)
    assert done.wait(timeout)
    runner.close()
    return events, exits


def test_streams_events_and_exit_code():
    events, exits = collect("ok")
    assert [e["event"] for e in events] == ["start", "result"]
    assert exits == [0]


def test_malformed_line_becomes_warning():
    events, _ = collect("garbage")
    warnings = [e for e in events if e["event"] == "log" and e["level"] == "WARNING"]
    assert warnings
    assert [e["event"] for e in events][-1] == "result"


def test_oversized_line_is_discarded_with_warning():
    events, _ = collect("huge")
    assert any(e.get("level") == "WARNING" for e in events if e["event"] == "log")


def test_non_utf8_line_decodes_lossily():
    events, _ = collect("nonutf8")
    assert any("message" in e for e in events if e["event"] == "log")


def test_exit_without_result_is_reported():
    events, exits = collect("silent")
    assert events == []
    assert exits == [3]


def test_cancel_signals_process_group():
    runner = OperationRunner([sys.executable, str(FAKE)], on_event=lambda e: None, on_exit=lambda c: None)
    runner.start("backup", "hang")
    runner.cancel()
    assert runner.wait(10) == 130
    runner.close()


def test_fake_engine_stream_drives_operation_state():
    from termux_sync.tui.models import apply_event, new_operation

    state = new_operation("backup")
    events, _ = collect("ok")
    for event in events:
        apply_event(state, event)
    assert state.status == "succeeded"
    assert state.result["verified"] is True


def test_confirm_after_exit_is_a_no_op():
    runner = OperationRunner(
        [sys.executable, str(FAKE)], on_event=lambda e: None, on_exit=lambda c: None
    )
    runner.start("backup", "silent")
    runner.wait(10)
    runner.confirm("RESTORE")
    runner.close()


def test_close_asks_the_engine_to_cancel_before_killing():
    events = []
    runner = OperationRunner(
        [sys.executable, str(FAKE)], on_event=events.append, on_exit=lambda c: None
    )
    runner.start("backup", "graceful")
    deadline = time.monotonic() + 5
    while not events and time.monotonic() < deadline:
        time.sleep(0.02)
    assert events, "fake engine never started"
    runner.close()
    assert any(event["event"] == "cancelled" for event in events)
