"""qutebrowser integration for the docent CLI.

This module imports qutebrowser internals. qutebrowser does not expose a stable
third-party plugin API yet, so keep the integration surface small and retest it
on qutebrowser upgrades.
"""

from __future__ import annotations

import json
import logging
import os
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional
from urllib.parse import urlsplit, urlunsplit

from qutebrowser.api import apitypes, cmdutils, message
from qutebrowser.misc import objects
from qutebrowser.qt.core import QProcess, QTimer
from qutebrowser.qt.widgets import QInputDialog
from qutebrowser.utils import objreg

from . import ui
from .profiles import ProfileError, ProfileInfo, has_requirement, load_profiles
from .request import Capture, build_request
from .stream import NdjsonParser, StreamError

__all__ = ["profiles", "refresh", "setup"]

_logger = logging.getLogger(__name__)

_CAPTURE_JS = """
(() => {
  const root = document.body || document.documentElement;
  const selection = window.getSelection ? window.getSelection().toString() : "";
  return {
    selection: selection,
    document: root ? root.innerText : "",
    url: location.href,
    title: document.title || ""
  };
})()
"""


class DocentError(Exception):
    """A user-facing preflight error."""


@dataclass(slots=True)
class Settings:
    """Plugin settings captured from ``setup``."""

    config: Any
    cmd: list[str]
    config_dir: str | None
    default_profile: str
    context_mode: str | None
    target_language: str | None
    open_in_background: bool
    keymaps: dict[str, str]
    custom_css: str | None


def request_arguments(
    settings: Settings,
    profile: str,
    target_language: str | None,
    context_mode: str | None,
) -> list[str]:
    """Build the argv passed to ``docent request``."""
    args = list(settings.cmd[1:])
    if settings.config_dir:
        args.extend(["--config-dir", settings.config_dir])
    args.extend(["request", "--profile", profile, "--stream", "--html"])
    if target_language:
        args.extend(["--target-language", target_language])
    if context_mode:
        args.extend(["--context-mode", context_mode])
    args.extend(["--source", "qutebrowser"])
    return args


def _warning_lines(stderr: bytes) -> list[str]:
    """Extracts the warnings the CLI logged to stderr, without log prefixes."""
    warnings = []
    for line in stderr.decode("utf-8", "replace").splitlines():
        if not line.startswith("[WARN"):
            continue
        _, separator, text = line.partition("] ")
        if separator and text:
            warnings.append(text)
    return warnings


@dataclass
class Session:
    """One request and the answer tab that displays it."""

    tabbed_browser: Any
    source_tab: apitypes.Tab
    answer_tab: apitypes.Tab
    profile: str
    answer_page: ui.AnswerPage
    settings: Settings
    parser: NdjsonParser = field(default_factory=NdjsonParser)
    process: QProcess | None = None
    stderr: bytearray = field(default_factory=bytearray)
    started: bool = False
    done: bool = False
    error: bool = False
    cancelled: bool = False
    html: str | None = None

    def start(
        self,
        request: dict[str, Any],
        target_language: str | None,
        context_mode: str | None,
    ) -> None:
        """Start the docent process and stream its output into the answer tab."""
        if self.cancelled:
            return

        args = request_arguments(
            self.settings,
            self.profile,
            target_language,
            context_mode,
        )

        process = QProcess()
        process.setProgram(self.settings.cmd[0])
        process.setArguments(args)
        process.readyReadStandardOutput.connect(self._read_stdout)
        process.readyReadStandardError.connect(self._read_stderr)
        process.finished.connect(self._finished)
        process.errorOccurred.connect(self._process_error)
        self.process = process

        process.start()
        if not process.waitForStarted(3000):
            self.error = True
            self.answer_page.set_error(f"could not start {self.settings.cmd[0]}")
            self._cleanup()
            return

        payload = json.dumps(request, ensure_ascii=False).encode("utf-8")
        if process.write(payload) < 0:
            self.error = True
            self.answer_page.set_error("could not write the request to docent")
            self._terminate()
            return
        process.closeWriteChannel()

    def on_tab_closed(self) -> None:
        """Cancel when the answer tab is closed."""
        self.cancel(close_tab=False)
        _sessions.pop(self.answer_tab.tab_id, None)
        global _last_answer_tab
        if _last_answer_tab is self.answer_tab:
            _last_answer_tab = None

    def on_navigation(self) -> None:
        """Cancel when the user navigates the answer tab away."""
        if self.started:
            self.cancel(close_tab=False)

    def cancel(self, *, close_tab: bool) -> None:
        """Stop the process and optionally close the answer tab."""
        if self.cancelled:
            return
        self.cancelled = True
        self.answer_page.close()
        self._terminate()
        if close_tab:
            try:
                self.tabbed_browser.close_tab(self.answer_tab)
            except Exception as exc:  # noqa: BLE001
                _logger.debug("could not close answer tab: %s", exc)

    def _terminate(self) -> None:
        if self.process is None:
            return
        if self.process.state() != QProcess.ProcessState.NotRunning:
            self.process.terminate()
            QTimer.singleShot(1000, self._kill)

    def _kill(self) -> None:
        if (
            self.process is not None
            and self.process.state() != QProcess.ProcessState.NotRunning
        ):
            self.process.kill()

    def _read_stdout(self) -> None:
        if self.process is None:
            return
        data = bytes(self.process.readAllStandardOutput().data())
        try:
            events = self.parser.feed(data)
        except StreamError as exc:
            self.error = True
            self.answer_page.set_error(str(exc))
            self._terminate()
            return
        for event in events:
            self._handle_event(event)

    def _read_stderr(self) -> None:
        if self.process is not None:
            self.stderr.extend(bytes(self.process.readAllStandardError().data()))

    def _handle_event(self, event: dict[str, Any]) -> None:
        kind = event.get("type")
        if kind == "delta":
            text = event.get("text")
            if isinstance(text, str):
                self.answer_page.append_text(text)
        elif kind == "html":
            body = event.get("html")
            if isinstance(body, str):
                self.html = body
        elif kind == "done":
            self.done = True
            model = event.get("model")
            self.answer_page.done(
                model if isinstance(model, str) else None,
                self.html,
            )
        elif kind == "error":
            self.error = True
            detail = event.get("message")
            self.answer_page.set_error(
                detail if isinstance(detail, str) else "unknown error"
            )
        else:
            self.error = True
            self.answer_page.set_error("invalid stream event")

    def _finished(self, exit_code: int, _exit_status: QProcess.ExitStatus) -> None:
        if self.cancelled:
            self._cleanup()
            return

        try:
            events = self.parser.flush()
        except StreamError as exc:
            self.error = True
            self.answer_page.set_error(str(exc))
            events = []
        for event in events:
            self._handle_event(event)

        if not self.error:
            if exit_code != 0:
                stderr = self.stderr.decode("utf-8", "replace").strip()
                self.answer_page.set_error(
                    stderr or f"docent exited with code {exit_code}"
                )
            else:
                for warning in _warning_lines(bytes(self.stderr)):
                    message.warning(f"docent: {warning}")
                if not self.done:
                    self.answer_page.done(None, self.html)
        self._cleanup()

    def _process_error(self, error: QProcess.ProcessError) -> None:
        if self.cancelled or self.error:
            return
        if error == QProcess.ProcessError.FailedToStart:
            self.error = True
            detail = self.process.errorString() if self.process is not None else ""
            name = self.settings.cmd[0]
            self.answer_page.set_error(f"could not start {name}: {detail}".rstrip(": "))
            self._cleanup()

    def _cleanup(self) -> None:
        if self.process is not None:
            self.process.deleteLater()
            self.process = None


_settings: Settings | None = None
_profiles: list[ProfileInfo] | None = None
_sessions: dict[int, Session] = {}
_last_answer_tab: apitypes.Tab | None = None


def setup(
    config: Any = None,
    *,
    cmd: str | Sequence[str] = "docent",
    config_dir: str | None = None,
    default_profile: str = "explain",
    context_mode: str | None = None,
    target_language: str | None = None,
    keymaps: Mapping[str, str] | None = None,
    open_in_background: bool = False,
    custom_css: str | None = None,
) -> None:
    """Configure the adapter and install opt-in keymaps.

    Must be called from ``config.py`` with qutebrowser's ``config`` object so
    keymaps can be registered with that object's ``bind`` method.
    """
    global _settings, _profiles

    if config is None:
        raise ValueError(
            "docent_qutebrowser.setup needs qutebrowser's config object; "
            "call docent_qutebrowser.setup(config, ...) from config.py"
        )
    if context_mode is not None and context_mode not in {"auto", "full", "none"}:
        raise ValueError("context_mode must be one of: auto, full, none")

    command = [cmd] if isinstance(cmd, str) else [str(part) for part in cmd]
    if not command or not command[0]:
        raise ValueError("cmd must name the docent executable")

    css_text = None
    if custom_css:
        css_path = Path(custom_css).expanduser()
        try:
            css_text = css_path.read_text(encoding="utf-8")
        except OSError as exc:
            raise ValueError(f"custom_css: could not read {css_path}: {exc}") from exc

    settings = Settings(
        config=config,
        cmd=command,
        config_dir=os.path.expanduser(config_dir) if config_dir else None,
        default_profile=default_profile,
        context_mode=context_mode,
        target_language=target_language,
        open_in_background=open_in_background,
        keymaps=dict(keymaps or {}),
        custom_css=css_text,
    )
    _settings = settings
    _profiles = None
    _bind_keymaps(settings)


def refresh() -> None:
    """Clear the cached profile list."""
    global _profiles
    _profiles = None


def profiles(force: bool = False) -> list[ProfileInfo]:
    """Return profile metadata, loading it from the CLI when necessary."""
    global _profiles
    settings = _require_settings()
    if _profiles is not None and not force:
        return _profiles
    _profiles = load_profiles(settings.cmd, config_dir=settings.config_dir)
    return _profiles


def _require_settings() -> Settings:
    if _settings is None:
        raise DocentError("call docent_qutebrowser.setup(config) in config.py first")
    return _settings


def _bind_keymaps(settings: Settings) -> None:
    for name, key in settings.keymaps.items():
        if not key:
            continue
        command = "docent-pick" if name == "pick" else f"docent {name}"
        for mode in ("normal", "caret"):
            settings.config.bind(key, command, mode=mode)


def _docent_command(
    tab: apitypes.Tab,
    profile: str = "explain",
    *,
    question: Optional[str] = None,  # noqa: UP045
    target_language: Optional[str] = None,  # noqa: UP045
    context_mode: Optional[str] = None,  # noqa: UP045
) -> None:
    """Run a docent profile against the current selection."""
    _run_request(
        tab,
        profile,
        question=question,
        target_language=target_language,
        context_mode=context_mode,
    )


def _pick_command(tab: apitypes.Tab) -> None:
    """Choose a docent profile and run it."""
    try:
        catalog = profiles(force=True)
    except (DocentError, ProfileError) as exc:
        message.error(f"docent: {exc}")
        return
    if not catalog:
        message.error("docent: no profiles found; run 'docent init'")
        return

    labels = [
        f"{profile.name}: {profile.description}"
        if profile.description
        else profile.name
        for profile in catalog
    ]
    choice, accepted = QInputDialog.getItem(
        tab.window(),
        "Docent profile",
        "Profile:",
        labels,
        0,
        False,
    )
    if not accepted:
        return
    for profile, label in zip(catalog, labels):
        if label == choice:
            _run_request(
                tab,
                profile.name,
                question=None,
                target_language=None,
                context_mode=None,
            )
            return


def _close_command(tab: apitypes.Tab) -> None:
    """Close the current answer tab, or the most recent one."""
    session = _sessions.get(tab.tab_id)
    if session is not None:
        session.cancel(close_tab=True)
        return

    target = _last_answer_tab
    if target is not None:
        try:
            if getattr(target, "pending_removal", False):
                target = None
        except RuntimeError:
            target = None
    if target is not None:
        try:
            tabbed_browser = objreg.get(
                "tabbed-browser",
                scope="window",
                window=target.win_id,
            )
            tabbed_browser.close_tab(target)
        except Exception as exc:  # noqa: BLE001
            _logger.debug("could not close answer tab: %s", exc)
        return

    message.info("docent: no answer tab")


def _refresh_command() -> None:
    """Reload docent profile metadata."""
    refresh()
    message.info("docent: profiles refreshed")


def _run_request(
    tab: apitypes.Tab,
    profile_name: str,
    *,
    question: str | None,
    target_language: str | None,
    context_mode: str | None,
) -> None:
    try:
        settings = _require_settings()
        catalog = profiles()
    except (DocentError, ProfileError) as exc:
        message.error(f"docent: {exc}")
        return

    profile = next((item for item in catalog if item.name == profile_name), None)
    if profile is None:
        message.error(f"docent: unknown profile '{profile_name}'")
        return

    def captured(result: Any) -> None:
        if getattr(tab, "pending_removal", False):
            return
        capture = _capture(result)
        if capture is None:
            message.error("docent: could not read this page")
            return
        if has_requirement(profile, "selection") and not capture.selection.strip():
            message.error("docent: select some text first")
            return
        if has_requirement(profile, "context") and not capture.document.strip():
            message.error("docent: the page has no text to send")
            return

        effective_question = question
        if has_requirement(profile, "question") and not (
            effective_question and effective_question.strip()
        ):
            text, accepted = QInputDialog.getText(
                tab.window(),
                "Docent",
                f"Question for {profile.name}:",
            )
            text = text or ""
            if not accepted or not text.strip():
                return
            effective_question = text

        request = build_request(capture, question=effective_question)
        _open_answer(
            tab,
            profile,
            request,
            target_language or settings.target_language,
            context_mode or settings.context_mode,
        )

    try:
        tab.run_js_async(_CAPTURE_JS, captured)
    except Exception as exc:  # noqa: BLE001
        message.error(f"docent: could not read this page: {exc}")


def _open_answer(
    source_tab: apitypes.Tab,
    profile: ProfileInfo,
    request: dict[str, Any],
    target_language: str | None,
    context_mode: str | None,
) -> None:
    global _last_answer_tab

    settings = _require_settings()
    try:
        tabbed_browser = objreg.get(
            "tabbed-browser",
            scope="window",
            window=source_tab.win_id,
        )
    except Exception as exc:  # noqa: BLE001
        message.error(f"docent: could not open an answer tab: {exc}")
        return

    try:
        answer_tab = tabbed_browser.tabopen(background=settings.open_in_background)
    except Exception as exc:  # noqa: BLE001
        message.error(f"docent: could not open an answer tab: {exc}")
        return

    page = ui.AnswerPage(answer_tab, profile.name)
    session = Session(
        tabbed_browser=tabbed_browser,
        source_tab=source_tab,
        answer_tab=answer_tab,
        profile=profile.name,
        answer_page=page,
        settings=settings,
    )
    _sessions[answer_tab.tab_id] = session
    _last_answer_tab = answer_tab

    def loaded(ok: bool) -> None:
        try:
            answer_tab.load_finished.disconnect(loaded)
        except (TypeError, RuntimeError):
            pass
        if session.cancelled:
            return
        if not ok:
            page.set_error("could not load the answer page")
            message.error("docent: could not load the answer page")
            return
        session.started = True
        answer_tab.load_started.connect(session.on_navigation)
        session.start(request, target_language, context_mode)

    answer_tab.shutting_down.connect(session.on_tab_closed)
    answer_tab.load_finished.connect(loaded)
    try:
        answer_tab.set_html(
            ui.build_shell(
                profile.name,
                _theme_colors(settings.config),
                settings.custom_css,
            )
        )
    except Exception as exc:  # noqa: BLE001
        page.set_error(f"could not open the answer tab: {exc}")
        message.error(f"docent: could not open the answer tab: {exc}")


def _capture(result: Any) -> Capture | None:
    if not isinstance(result, dict):
        return None

    def text(name: str) -> str:
        value = result.get(name)
        return value if isinstance(value, str) else ""

    return Capture(
        selection=text("selection"),
        document=text("document"),
        file=_file_label(text("url"), text("title")),
    )


def _file_label(url: str, title: str) -> str:
    clean = _clean_url(url)
    if title and clean:
        return f"{title} ({clean})"
    return clean or title


def _clean_url(url: str) -> str:
    """Remove query and fragment so tokens do not reach the model."""
    if not url:
        return ""
    try:
        parts = urlsplit(url)
    except ValueError:
        return url
    if not parts.scheme or not parts.netloc:
        return url
    netloc = parts.netloc.rsplit("@", 1)[-1]
    return urlunsplit((parts.scheme, netloc, parts.path, "", ""))


def _theme_colors(config: Any) -> dict[str, str]:
    """Return the configured answer-tab colors that are available."""
    colors: dict[str, str] = {}
    for name, key in (
        ("bg", "colors.statusbar.normal.bg"),
        ("fg", "colors.statusbar.normal.fg"),
        ("error", "colors.messages.error.fg"),
    ):
        try:
            value = config.get(key)
        except Exception as exc:  # noqa: BLE001
            _logger.debug("could not read %s: %s", key, exc)
            continue
        if isinstance(value, str):
            colors[name] = value
    return colors


def _register(
    handler: Any,
    name: str,
    arguments: tuple[tuple[str, dict[str, Any]], ...] = (),
) -> Any:
    for argname, kwargs in arguments:
        handler = cmdutils.argument(argname, **kwargs)(handler)
    if name not in objects.commands:
        handler = cmdutils.register(name=name)(handler)
    return handler


_docent_command = _register(
    _docent_command,
    "docent",
    (
        ("tab", {"value": cmdutils.Value.cur_tab}),
        ("context_mode", {"choices": ["auto", "full", "none"]}),
    ),
)
_pick_command = _register(
    _pick_command, "docent-pick", (("tab", {"value": cmdutils.Value.cur_tab}),)
)
_close_command = _register(
    _close_command, "docent-close", (("tab", {"value": cmdutils.Value.cur_tab}),)
)
_refresh_command = _register(_refresh_command, "docent-refresh")
