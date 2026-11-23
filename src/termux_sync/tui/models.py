"""Operation state for the cockpit: stages, weighted progress, terminal state."""

import time
from collections import deque
from dataclasses import dataclass, field

BACKUP_STAGES = {
    "preflight": 2,
    "packages": 5,
    "archive:home": 50,
    "archive:usr_etc": 13,
    "manifest": 5,
    "upload": 20,
    "verify": 5,
}

RESTORE_STAGES = {
    "plan": 5,
    "download": 25,
    "verify": 10,
    "extract:home": 35,
    "extract:usr_etc": 10,
    "packages": 10,
    "cleanup": 5,
}

STAGE_LABELS = {
    "preflight": "Preflight",
    "packages": "Packages",
    "archive:home": "Home",
    "archive:usr_etc": "Config",
    "manifest": "Manifest",
    "upload": "Upload",
    "verify": "Verify",
    "plan": "Plan",
    "download": "Download",
    "extract:home": "Home",
    "extract:usr_etc": "Config",
    "cleanup": "Cleanup",
}


@dataclass
class Stage:
    id: str
    label: str
    state: str = "pending"
    weight: int = 0
    current: int | None = None
    total: int | None = None


@dataclass
class OperationState:
    command: str
    stages: list[Stage]
    status: str = "running"
    name: str = ""
    storage: str = ""
    verified: bool | None = None
    verify_detail: str = ""
    result: dict | None = None
    failure: dict | None = None
    logs: deque[str] = field(default_factory=lambda: deque(maxlen=2000))
    started_at: float = field(default_factory=time.monotonic)

    def overall_percent(self) -> float:
        total_weight = sum(stage.weight for stage in self.stages)
        if total_weight <= 0:
            return 0.0
        done = 0.0
        for stage in self.stages:
            if stage.state == "complete":
                done += stage.weight
            elif stage.state == "running" and stage.total:
                fraction = min(max((stage.current or 0) / stage.total, 0.0), 1.0)
                done += stage.weight * fraction
        return round(done / total_weight * 100.0, 2)

    def current_stage(self) -> Stage | None:
        for stage in self.stages:
            if stage.state == "running":
                return stage
        return None


def new_operation(command: str) -> OperationState:
    weights = RESTORE_STAGES if command == "restore" else BACKUP_STAGES
    stages = [
        Stage(id=stage_id, label=STAGE_LABELS.get(stage_id, stage_id.title()), weight=weight)
        for stage_id, weight in weights.items()
    ]
    return OperationState(command=command, stages=stages)


def _rescale(stages: list[Stage]) -> None:
    total = sum(stage.weight for stage in stages)
    if total <= 0:
        return
    exact = [stage.weight * 100 / total for stage in stages]
    weights = [int(value) for value in exact]
    remainder = 100 - sum(weights)
    order = sorted(range(len(stages)), key=lambda i: exact[i] - weights[i], reverse=True)
    for index in order[:remainder]:
        weights[index] += 1
    for stage, weight in zip(stages, weights):
        stage.weight = weight


def _find(state: OperationState, stage_id: str) -> Stage | None:
    for stage in state.stages:
        if stage.id == stage_id:
            return stage
    return None


def apply_event(state: OperationState, event: dict) -> None:
    kind = event.get("event")
    if kind == "start":
        state.name = event.get("name", state.name)
        state.storage = event.get("storage", state.storage)
    elif kind == "stage":
        stage = _find(state, str(event.get("id", "")))
        if stage is None:
            return
        incoming = str(event.get("state", ""))
        if incoming == "skipped":
            state.stages.remove(stage)
            _rescale(state.stages)
            return
        stage.label = event.get("label", stage.label)
        stage.state = "running" if incoming == "start" else incoming
    elif kind == "progress":
        stage = _find(state, str(event.get("stage", "")))
        if stage is None:
            return
        if event.get("current") is not None:
            stage.current = event["current"]
        if event.get("total") is not None:
            stage.total = event["total"]
    elif kind == "log":
        state.logs.append(f"[{event.get('level', 'INFO')}] {event.get('message', '')}")
    elif kind == "verify":
        verify_state = event.get("state")
        state.verified = True if verify_state == "verified" else (False if verify_state == "failed" else None)
        state.verify_detail = str(event.get("details", ""))
    elif kind == "result":
        state.status = "succeeded" if event.get("ok") else "failed"
        state.result = event
    elif kind == "error":
        state.status = "failed"
        state.failure = event
    elif kind == "cancelled":
        state.status = "cancelled"
