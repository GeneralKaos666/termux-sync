"""Reusable cockpit widgets."""

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.widgets import ProgressBar, RichLog, Static


class RenderableStatic(Static):
    """Static with the older ``renderable`` alias, which the tests and cards use."""

    @property
    def renderable(self):
        return self.content


class StageStepper(RenderableStatic):
    """One line per stage, or a single current-stage line on narrow terminals."""


class LogView(RichLog):
    """RichLog kept as its own type so the cockpit can style it by class."""


class GaugeRow(Vertical):
    """Overall progress plus the progress of the stage currently running."""

    def compose(self) -> ComposeResult:
        yield ProgressBar(total=100, show_eta=False, id="overall")
        yield ProgressBar(total=100, show_eta=False, id="stage")
        yield Static("", id="gauge-detail")
