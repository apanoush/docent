"""Tests for qutebrowser-facing helpers that do not require a browser."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import docent
import pytest
from docent import plugin, stylesheet, ui
from docent.profiles import ProfileError, ProfileInfo
from docent.request import Capture


class FakeConfig:
    """Minimal stand-in for qutebrowser's config API."""

    def __init__(self) -> None:
        self.bound: list[tuple[str, str, str]] = []

    def bind(self, key: str, command: str, mode: str = "normal") -> None:
        self.bound.append((key, command, mode))

    def get(self, name: str) -> Any:
        colors = {
            "colors.statusbar.normal.bg": "#101010",
            "colors.statusbar.normal.fg": "#f0f0f0",
            "colors.messages.error.fg": "#ff0000",
        }
        return colors.get(name)


class FakeTab:
    """Minimal tab recording injected JavaScript."""

    def __init__(self) -> None:
        self.tab_id = 1
        self.win_id = 1
        self.pending_removal = False
        self.codes: list[str] = []

    def run_js_async(self, code: str) -> None:
        self.codes.append(code)

    def window(self) -> Any:
        return None


class FakePage:
    """Minimal answer page for Session tests."""

    def __init__(self) -> None:
        self.text: list[str] = []
        self.errors: list[str] = []
        self.models: list[str | None] = []
        self.done_calls: list[tuple[str | None, str | None]] = []
        self.finish_calls: list[str | None] = []
        self.finished = 0
        self.closed = False

    def append_text(self, text: str) -> None:
        self.text.append(text)

    def set_error(self, message: str) -> None:
        self.errors.append(message)

    def done(self, model: str | None = None, html_body: str | None = None) -> None:
        self.models.append(model)
        self.done_calls.append((model, html_body))

    def finish(self, html_body: str | None = None) -> None:
        self.finished += 1
        self.finish_calls.append(html_body)

    def close(self) -> None:
        self.closed = True


@pytest.fixture(autouse=True)
def reset_plugin() -> Any:
    plugin._settings = None
    plugin._profiles = None
    plugin._sessions.clear()
    plugin._last_answer_tab = None
    yield
    plugin._settings = None
    plugin._profiles = None
    plugin._sessions.clear()
    plugin._last_answer_tab = None


def _settings() -> plugin.Settings:
    return plugin.Settings(
        config=FakeConfig(),
        cmd=["docent"],
        config_dir=None,
        default_profile="explain",
        context_mode=None,
        target_language=None,
        open_in_background=False,
        keymaps={},
        custom_css=None,
    )


def test_setup_requires_config_object() -> None:
    with pytest.raises(ValueError, match="config object"):
        plugin.setup()


def test_setup_rejects_invalid_context_mode() -> None:
    with pytest.raises(ValueError, match="context_mode"):
        plugin.setup(FakeConfig(), context_mode="everything")


def test_setup_binds_keymaps_in_normal_and_caret_modes() -> None:
    config = FakeConfig()

    plugin.setup(
        config,
        keymaps={"explain": ",de", "pick": ",dp", "translate": ""},
    )

    assert config.bound == [
        (",de", "docent explain", "normal"),
        (",de", "docent explain", "caret"),
        (",dp", "docent-pick", "normal"),
        (",dp", "docent-pick", "caret"),
    ]


def test_setup_normalizes_command_and_config_dir() -> None:
    config = FakeConfig()

    plugin.setup(config, cmd=["docent", "--debug"], config_dir="~/docent")

    assert plugin._settings is not None
    assert plugin._settings.cmd == ["docent", "--debug"]
    assert plugin._settings.config_dir is not None
    assert not plugin._settings.config_dir.startswith("~")


def test_setup_rejects_empty_command() -> None:
    with pytest.raises(ValueError, match="cmd"):
        plugin.setup(FakeConfig(), cmd=[])


def test_clean_url_removes_query_and_fragment() -> None:
    assert (
        plugin._clean_url("https://example.com/a?token=secret#part")
        == "https://example.com/a"
    )


def test_clean_url_removes_userinfo() -> None:
    assert (
        plugin._clean_url("https://user:secret@example.com/a?token=x")
        == "https://example.com/a"
    )


def test_clean_url_keeps_relative_and_special_urls() -> None:
    assert plugin._clean_url("notes.md") == "notes.md"
    assert plugin._clean_url("about:blank") == "about:blank"
    assert plugin._clean_url("") == ""


def test_file_label_prefers_title_and_clean_url() -> None:
    assert (
        plugin._file_label("https://example.com/a?x=1", "Example")
        == "Example (https://example.com/a)"
    )


def test_file_label_falls_back_to_url_or_title() -> None:
    assert plugin._file_label("https://example.com/a", "") == "https://example.com/a"
    assert plugin._file_label("", "Example") == "Example"
    assert plugin._file_label("", "") == ""


def test_capture_ignores_non_string_values() -> None:
    result = plugin._capture(
        {"selection": 3, "document": None, "url": [], "title": "Title"}
    )

    assert result == Capture(selection="", document="", file="Title")


def test_capture_rejects_non_dict_results() -> None:
    assert plugin._capture(None) is None
    assert plugin._capture("text") is None


def test_answer_page_append_uses_a_text_node() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.append_text('hello "world"\n')

    assert len(tab.codes) == 1
    assert "createTextNode" in tab.codes[0]
    assert 'hello \\"world\\"' in tab.codes[0]
    assert not page.finished
    assert not page.failed


def test_answer_page_error_and_done_stop_updates() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.set_error("boom")
    page.append_text("ignored")

    assert page.failed
    assert page.finished
    assert len(tab.codes) == 1
    assert "error: boom" in tab.codes[0]


def test_answer_page_done_updates_title_and_stops_updates() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.done("test-model")
    page.append_text("ignored")

    assert page.finished
    assert len(tab.codes) == 1
    assert "docent: explain - test-model" in tab.codes[0]


def test_answer_page_done_uses_the_rendered_html() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.append_text("# Title\n\nBody with **bold**.")
    page.done("test-model", "<h1>Title</h1>\n<p>Body with <strong>bold</strong>.</p>")

    rendered = tab.codes[-1]
    assert "<h1>Title</h1>" in rendered
    assert "<strong>bold</strong>" in rendered
    assert "docent: explain - test-model" in rendered


def test_answer_page_finish_uses_the_rendered_html() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.append_text("- one")
    page.finish("<ul><li>one</li></ul>")

    rendered = tab.codes[-1]
    assert "<ul><li>one</li></ul>" in rendered
    assert "(no output)" not in rendered


def test_answer_page_does_not_insert_untrusted_html() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.append_text("<script>alert(1)</script>")
    page.done(None, "&lt;script&gt;alert(1)&lt;/script&gt;")

    rendered = tab.codes[-1]
    assert "<script>" not in rendered
    assert "&lt;script&gt;" in rendered


def test_answer_page_finish_without_html_keeps_the_streamed_text() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.append_text("- one")
    page.finish()

    assert page.finished
    assert len(tab.codes) == 2
    assert "docent: explain" in tab.codes[-1]
    assert "(no output)" not in tab.codes[-1]


def test_answer_page_close_stops_updates() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.close()
    page.append_text("ignored")

    assert page.closed
    assert tab.codes == []


def test_answer_page_stops_when_tab_is_pending_removal() -> None:
    tab = FakeTab()
    tab.pending_removal = True
    page = ui.AnswerPage(tab, "explain")

    page.append_text("ignored")

    assert page.closed
    assert tab.codes == []


def test_build_shell_escapes_profile_and_uses_colors() -> None:
    shell = ui.build_shell(
        '<script>alert("x")</script>',
        {"bg": "#000000", "fg": "#ffffff", "error": "#ff0000"},
    )

    assert "&lt;script&gt;" in shell
    assert "<script>alert" not in shell
    assert "#000000" in shell
    assert "#ffffff" in shell
    assert "#ff0000" in shell
    assert "d close" in shell


def test_build_shell_rejects_unsafe_colors() -> None:
    shell = ui.build_shell("explain", {"bg": "red; } body { display: none"})

    assert "display: none" not in shell


def test_stylesheet_is_exported() -> None:
    css = stylesheet()

    assert "--docent-font-body" in css
    assert "--docent-font-code" in css
    assert ".markdown" in css


def test_build_shell_includes_the_default_stylesheet() -> None:
    shell = ui.build_shell("explain")

    assert ui.stylesheet() in shell


def test_build_shell_accepts_label_hint_and_script() -> None:
    shell = ui.build_shell(
        "explain",
        label="docent: explain - paper.pdf",
        hint=None,
        script="console.log('sse')",
    )

    assert "docent: explain - paper.pdf" in shell
    assert "d close" not in shell
    assert "<script>\nconsole.log('sse')\n</script>" in shell


def test_build_shell_composes_stylesheet_palette_and_custom_css() -> None:
    custom = ".markdown { color: red; }"

    shell = ui.build_shell("explain", {"bg": "#000000"}, custom)

    default_at = shell.index(ui.stylesheet())
    palette_at = shell.index("--docent-bg: #000000;")
    custom_at = shell.index(custom)
    assert default_at < palette_at < custom_at


def test_setup_reads_custom_css(tmp_path: Path) -> None:
    path = tmp_path / "answer.css"
    path.write_text(".markdown { color: red; }", encoding="utf-8")

    plugin.setup(FakeConfig(), custom_css=str(path))

    assert plugin._settings is not None
    assert plugin._settings.custom_css == ".markdown { color: red; }"


def test_setup_rejects_a_missing_custom_css(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="custom_css"):
        plugin.setup(FakeConfig(), custom_css=str(tmp_path / "missing.css"))


def test_theme_colors_skip_unavailable_values() -> None:
    class FailingConfig:
        def get(self, name: str) -> Any:
            raise ValueError(name)

    assert plugin._theme_colors(FailingConfig()) == {}


def test_session_handles_delta_done_and_error_events() -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="explain",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )

    session._handle_event({"type": "delta", "text": "hello"})
    session._handle_event({"type": "html", "html": "<p>hello</p>"})
    session._handle_event({"type": "done", "model": "test-model"})
    session._handle_event({"type": "error", "message": "boom"})
    session._handle_event({"type": "unknown"})

    assert page.text == ["hello"]
    assert page.models == ["test-model"]
    assert page.done_calls == [("test-model", "<p>hello</p>")]
    assert page.errors == ["boom", "invalid stream event"]
    assert session.done
    assert session.error


def test_session_finishes_cleanly_on_success_exit() -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="explain",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )

    session._finished(0, None)  # type: ignore[arg-type]

    assert page.finished == 1
    assert page.finish_calls == [None]
    assert page.errors == []


def test_session_passes_rendered_html_to_finish() -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="explain",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )
    session._handle_event({"type": "html", "html": "<p>hello</p>"})

    session._finished(0, None)  # type: ignore[arg-type]

    assert page.finish_calls == ["<p>hello</p>"]


def test_session_warns_on_stderr_after_success(monkeypatch: pytest.MonkeyPatch) -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="summarize",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )
    session.stderr.extend(
        b"[WARN  docent::cli] document context truncated to 40 characters\n"
    )
    warnings: list[str] = []
    monkeypatch.setattr(plugin.message, "warning", warnings.append)

    session._finished(0, None)  # type: ignore[arg-type]

    assert warnings == ["docent: document context truncated to 40 characters"]
    assert page.finished == 1
    assert page.errors == []


def test_session_ignores_non_warning_stderr_after_success(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="summarize",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )
    session.stderr.extend(b"error: something else\n")
    warnings: list[str] = []
    monkeypatch.setattr(plugin.message, "warning", warnings.append)

    session._finished(0, None)  # type: ignore[arg-type]

    assert warnings == []
    assert page.finished == 1


def test_session_reports_stderr_on_failed_exit() -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="explain",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )
    session.stderr.extend(b"provider failed\n")

    session._finished(3, None)  # type: ignore[arg-type]

    assert page.errors == ["provider failed"]
    assert page.finished == 0


def test_session_does_not_finish_after_an_error_event() -> None:
    page = FakePage()
    session = plugin.Session(
        tabbed_browser=None,
        source_tab=FakeTab(),
        answer_tab=FakeTab(),
        profile="explain",
        page=page,  # type: ignore[arg-type]
        settings=_settings(),
    )
    session._handle_event({"type": "error", "message": "boom"})

    session._finished(3, None)  # type: ignore[arg-type]

    assert page.errors == ["boom"]
    assert page.finished == 0


class CaptureTab(FakeTab):
    """Tab that returns a capture result when JavaScript runs."""

    def __init__(self, result: Any) -> None:
        super().__init__()
        self.result = result

    def run_js_async(self, code: str, callback: Any = None) -> None:
        self.codes.append(code)
        if callback is not None:
            callback(self.result)


def test_run_request_prompts_for_a_question(monkeypatch: Any) -> None:
    profile = ProfileInfo("ask", None, ("selection", "question"))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    opened: dict[str, Any] = {}

    def open_answer(
        source: Any,
        selected: ProfileInfo,
        request: dict[str, Any],
        target: str | None,
        context: str | None,
    ) -> None:
        opened.update(
            source=source,
            profile=selected.name,
            request=request,
            target=target,
            context=context,
        )

    monkeypatch.setattr(plugin, "_open_answer", open_answer)
    monkeypatch.setattr(
        plugin.QInputDialog, "getText", lambda *args, **kwargs: ("Why?", True)
    )
    tab = CaptureTab(
        {
            "selection": "passage",
            "document": "page text",
            "url": "https://example.com/x?token=secret",
            "title": "Title",
        }
    )

    plugin._run_request(
        tab,
        "ask",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert opened["profile"] == "ask"
    assert opened["request"]["question"] == "Why?"
    assert opened["request"]["file"] == "Title (https://example.com/x)"


def test_run_request_rejects_empty_selection(monkeypatch: Any) -> None:
    profile = ProfileInfo("explain", None, ("selection",))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(
        plugin.message, "error", lambda *args, **kwargs: errors.append(args[0])
    )
    monkeypatch.setattr(plugin, "_open_answer", lambda *args: pytest.fail("opened"))
    tab = CaptureTab({"selection": "  ", "document": "page"})

    plugin._run_request(
        tab,
        "explain",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert errors == ["docent: select some text first"]


def test_run_request_reports_unknown_profile(monkeypatch: Any) -> None:
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [])
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(
        plugin.message, "error", lambda *args, **kwargs: errors.append(args[0])
    )

    plugin._run_request(
        CaptureTab({}),
        "missing",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert errors == ["docent: unknown profile 'missing'"]


def test_run_request_aborts_when_question_prompt_is_cancelled(monkeypatch: Any) -> None:
    profile = ProfileInfo("ask", None, ("question",))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    monkeypatch.setattr(
        plugin.QInputDialog, "getText", lambda *args, **kwargs: ("", False)
    )
    monkeypatch.setattr(plugin, "_open_answer", lambda *args: pytest.fail("opened"))

    plugin._run_request(
        CaptureTab({"selection": "passage", "document": "page"}),
        "ask",
        question=None,
        target_language=None,
        context_mode=None,
    )


def test_run_request_reports_profile_load_errors(monkeypatch: Any) -> None:
    def fail(force: bool = False) -> list[ProfileInfo]:
        raise ProfileError("no profiles")

    monkeypatch.setattr(plugin, "profiles", fail)
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(
        plugin.message, "error", lambda *args, **kwargs: errors.append(args[0])
    )

    plugin._run_request(
        CaptureTab({}),
        "explain",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert errors == ["docent: no profiles"]


def test_pick_command_runs_the_selected_profile(monkeypatch: Any) -> None:
    profiles = [
        ProfileInfo("explain", "Explain a passage", ("selection",)),
        ProfileInfo("ask", None, ("question",)),
    ]
    monkeypatch.setattr(plugin, "profiles", lambda force=False: profiles)
    plugin._settings = _settings()
    monkeypatch.setattr(
        plugin.QInputDialog,
        "getItem",
        lambda *args, **kwargs: ("ask", True),
    )
    ran: list[str] = []
    monkeypatch.setattr(
        plugin,
        "_run_request",
        lambda tab, name, **kwargs: ran.append(name),
    )

    plugin._pick_command(FakeTab())

    assert ran == ["ask"]


def test_request_arguments_tags_the_source() -> None:
    args = plugin.request_arguments(_settings(), "explain", "French", "full")

    assert args == [
        "request",
        "--profile",
        "explain",
        "--stream",
        "--html",
        "--target-language",
        "French",
        "--context-mode",
        "full",
        "--source",
        "qutebrowser",
    ]


def test_request_arguments_include_config_dir() -> None:
    settings = _settings()
    settings.config_dir = "/tmp/docent"

    args = plugin.request_arguments(settings, "ask", None, None)

    assert args == [
        "--config-dir",
        "/tmp/docent",
        "request",
        "--profile",
        "ask",
        "--stream",
        "--html",
        "--source",
        "qutebrowser",
    ]


class FakeSignal:
    """Record the callbacks connected to a Qt signal."""

    def __init__(self) -> None:
        self.handlers: list[Any] = []

    def connect(self, handler: Any) -> None:
        self.handlers.append(handler)

    def disconnect(self, handler: Any) -> None:
        self.handlers.remove(handler)

    def emit(self, *args: Any) -> None:
        for handler in list(self.handlers):
            handler(*args)


class FakeAnswerTab(FakeTab):
    """Answer tab exposing the signals and HTML setter the plugin uses."""

    def __init__(self) -> None:
        super().__init__()
        self.win_id = 2
        self.html: list[str] = []
        self.load_finished = FakeSignal()
        self.load_started = FakeSignal()
        self.shutting_down = FakeSignal()

    def set_html(self, html: str) -> None:
        self.html.append(html)


class BrokenAnswerTab(FakeAnswerTab):
    """Answer tab that rejects the answer shell."""

    def set_html(self, html: str) -> None:
        raise RuntimeError("shell")


class FakeTabbedBrowser:
    """Record tab opens and closes in place of qutebrowser's registry."""

    def __init__(
        self,
        answer_tab: FakeAnswerTab | None = None,
        *,
        tabopen_error: Exception | None = None,
        close_error: Exception | None = None,
    ) -> None:
        self.answer_tab = answer_tab or FakeAnswerTab()
        self.opened: list[bool] = []
        self.closed: list[Any] = []
        self._tabopen_error = tabopen_error
        self._close_error = close_error

    def tabopen(self, background: bool = False) -> FakeAnswerTab:
        if self._tabopen_error is not None:
            raise self._tabopen_error
        self.opened.append(background)
        return self.answer_tab

    def close_tab(self, tab: Any) -> None:
        if self._close_error is not None:
            raise self._close_error
        self.closed.append(tab)


class _Bytes:
    """Stand in for the QByteArray returned by a QProcess read."""

    def __init__(self, data: bytes) -> None:
        self._data = data

    def data(self) -> bytes:
        return self._data


class FakeProcess:
    """Minimal QProcess used to exercise ``Session.start`` without Qt."""

    class ProcessState:
        NotRunning = 0
        Running = 1

    class ProcessError:
        FailedToStart = 0
        Crashed = 1

    def __init__(
        self,
        *,
        starts: bool = True,
        write_result: int = 0,
        state: int = 1,
        stdout: bytes = b"",
        stderr: bytes = b"",
        error_string: str = "",
    ) -> None:
        self.program = ""
        self.arguments: list[str] = []
        self.started = False
        self.closed_write = False
        self.deleted = False
        self.terminated = False
        self.killed = False
        self.written: list[bytes] = []
        self._starts = starts
        self._write_result = write_result
        self._state = state
        self._stdout = stdout
        self._stderr = stderr
        self._error_string = error_string
        self.readyReadStandardOutput = FakeSignal()
        self.readyReadStandardError = FakeSignal()
        self.finished = FakeSignal()
        self.errorOccurred = FakeSignal()

    def setProgram(self, program: str) -> None:
        self.program = program

    def setArguments(self, arguments: list[str]) -> None:
        self.arguments = arguments

    def start(self) -> None:
        self.started = True

    def waitForStarted(self, timeout: int) -> bool:
        return self._starts

    def write(self, payload: bytes) -> int:
        if self._write_result < 0:
            return self._write_result
        self.written.append(payload)
        return len(payload)

    def closeWriteChannel(self) -> None:
        self.closed_write = True

    def state(self) -> int:
        return self._state

    def terminate(self) -> None:
        self.terminated = True

    def kill(self) -> None:
        self.killed = True

    def deleteLater(self) -> None:
        self.deleted = True

    def readAllStandardOutput(self) -> _Bytes:
        return _Bytes(self._stdout)

    def readAllStandardError(self) -> _Bytes:
        return _Bytes(self._stderr)

    def errorString(self) -> str:
        return self._error_string


def _install_fake_process(
    monkeypatch: pytest.MonkeyPatch, **options: Any
) -> list[FakeProcess]:
    created: list[FakeProcess] = []

    class TrackingProcess(FakeProcess):
        def __init__(self) -> None:
            super().__init__(**options)
            created.append(self)

    monkeypatch.setattr(plugin, "QProcess", TrackingProcess)
    return created


def _install_fake_timer(monkeypatch: pytest.MonkeyPatch) -> list[tuple[int, Any]]:
    scheduled: list[tuple[int, Any]] = []

    class FakeTimer:
        @staticmethod
        def singleShot(msec: int, callback: Any) -> None:
            scheduled.append((msec, callback))

    monkeypatch.setattr(plugin, "QTimer", FakeTimer)
    return scheduled


def _session(
    page: FakePage | None = None,
    *,
    tabbed_browser: Any = None,
) -> plugin.Session:
    return plugin.Session(
        tabbed_browser=tabbed_browser,
        source_tab=FakeTab(),
        answer_tab=FakeAnswerTab(),
        profile="explain",
        page=page or FakePage(),  # type: ignore[arg-type]
        settings=_settings(),
    )


def _profile() -> ProfileInfo:
    return ProfileInfo("explain", "Explain a passage", ("selection",))


def test_session_start_spawns_docent_and_writes_the_request(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    created = _install_fake_process(monkeypatch)
    session = _session()

    session.start({"selection": "passage"}, "French", "full")

    process = created[0]
    assert process.program == "docent"
    assert process.arguments == [
        "request",
        "--profile",
        "explain",
        "--stream",
        "--html",
        "--target-language",
        "French",
        "--context-mode",
        "full",
        "--source",
        "qutebrowser",
    ]
    assert process.started
    assert process.written == [b'{"selection": "passage"}']
    assert process.closed_write
    assert session.process is process


def test_session_start_reports_a_process_that_does_not_start(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    created = _install_fake_process(monkeypatch, starts=False)
    page = FakePage()
    session = _session(page)

    session.start({}, None, None)

    assert page.errors == ["could not start docent"]
    assert session.error
    assert session.process is None
    assert created[0].deleted


def test_session_start_reports_a_short_write(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_fake_timer(monkeypatch)
    created = _install_fake_process(monkeypatch, write_result=-1)
    page = FakePage()
    session = _session(page)

    session.start({}, None, None)

    assert page.errors == ["could not write the request to docent"]
    assert session.error
    assert created[0].written == []
    assert created[0].terminated


def test_session_start_ignores_a_cancelled_session(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    created = _install_fake_process(monkeypatch)
    session = _session()
    session.cancelled = True

    session.start({}, None, None)

    assert created == []
    assert session.process is None


def test_session_reads_stream_events_from_stdout(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _install_fake_process(
        monkeypatch,
        stdout=b'{"type":"delta","text":"hi"}\n{"type":"done","model":"m"}\n',
    )
    page = FakePage()
    session = _session(page)
    session.process = plugin.QProcess()

    session._read_stdout()

    assert page.text == ["hi"]
    assert page.models == ["m"]
    assert session.done
    assert not session.error


def test_session_reports_an_invalid_stream(monkeypatch: pytest.MonkeyPatch) -> None:
    _install_fake_timer(monkeypatch)
    _install_fake_process(monkeypatch, stdout=b"not json\n")
    page = FakePage()
    session = _session(page)
    process = plugin.QProcess()
    session.process = process

    session._read_stdout()

    assert session.error
    assert page.errors and page.errors[0].startswith("invalid stream event:")
    assert process.terminated


def test_session_reads_stdout_only_with_a_process() -> None:
    session = _session()
    session.process = None

    session._read_stdout()
    session._read_stderr()

    assert session.error is False
    assert session.stderr == bytearray()


def test_session_collects_stderr() -> None:
    session = _session()
    session.process = FakeProcess(stderr=b"[WARN  docent::cli] cut\n")

    session._read_stderr()

    assert session.stderr == bytearray(b"[WARN  docent::cli] cut\n")


def test_session_on_tab_closed_cancels_and_forgets_the_tab(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _install_fake_timer(monkeypatch)
    page = FakePage()
    tabbed = FakeTabbedBrowser()
    session = _session(page, tabbed_browser=tabbed)
    plugin._sessions[session.answer_tab.tab_id] = session
    plugin._last_answer_tab = session.answer_tab

    session.on_tab_closed()

    assert session.cancelled
    assert page.closed
    assert tabbed.closed == []
    assert plugin._sessions == {}
    assert plugin._last_answer_tab is None


def test_session_on_navigation_cancels_only_a_started_session(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _install_fake_timer(monkeypatch)
    page = FakePage()
    session = _session(page)

    session.on_navigation()
    assert not session.cancelled

    session.started = True
    session.on_navigation()

    assert session.cancelled
    assert page.closed


def test_session_cancel_closes_the_tab_once() -> None:
    page = FakePage()
    tabbed = FakeTabbedBrowser()
    session = _session(page, tabbed_browser=tabbed)

    session.cancel(close_tab=True)
    session.cancel(close_tab=True)

    assert page.closed
    assert tabbed.closed == [session.answer_tab]


def test_session_cancel_survives_a_missing_tabbed_browser() -> None:
    tabbed = FakeTabbedBrowser(close_error=RuntimeError("gone"))
    session = _session(tabbed_browser=tabbed)

    session.cancel(close_tab=True)

    assert session.cancelled


def test_session_reports_a_process_that_failed_to_start(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(plugin, "QProcess", FakeProcess)
    page = FakePage()
    session = _session(page)
    session.process = FakeProcess(error_string="no such file")

    session._process_error(FakeProcess.ProcessError.FailedToStart)

    assert page.errors == ["could not start docent: no such file"]
    assert session.error
    assert session.process is None


def test_session_ignores_other_process_errors(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(plugin, "QProcess", FakeProcess)
    page = FakePage()
    session = _session(page)
    process = FakeProcess()
    session.process = process

    session._process_error(FakeProcess.ProcessError.Crashed)

    assert page.errors == []
    assert session.process is process


def test_session_ignores_process_errors_when_finished(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(plugin, "QProcess", FakeProcess)
    page = FakePage()
    session = _session(page)
    session.cancelled = True

    session._process_error(FakeProcess.ProcessError.FailedToStart)

    session.cancelled = False
    session.error = True
    session._process_error(FakeProcess.ProcessError.FailedToStart)

    assert page.errors == []


def test_session_terminate_schedules_a_kill_for_a_running_process(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    scheduled = _install_fake_timer(monkeypatch)
    _install_fake_process(monkeypatch, state=FakeProcess.ProcessState.Running)
    session = _session()
    process = plugin.QProcess()
    session.process = process

    session._terminate()

    assert process.terminated
    assert len(scheduled) == 1
    scheduled[0][1]()
    assert process.killed


def test_session_terminate_leaves_a_stopped_process_alone(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    scheduled = _install_fake_timer(monkeypatch)
    _install_fake_process(monkeypatch, state=FakeProcess.ProcessState.NotRunning)
    session = _session()
    process = plugin.QProcess()
    session.process = process

    session._terminate()

    assert not process.terminated
    assert scheduled == []


def test_close_command_cancels_the_current_session(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    cancelled: list[bool] = []
    monkeypatch.setattr(
        plugin.Session,
        "cancel",
        lambda self, close_tab: cancelled.append(close_tab),
    )
    session = _session()
    plugin._sessions[session.answer_tab.tab_id] = session
    tab = FakeTab()
    tab.tab_id = session.answer_tab.tab_id

    plugin._close_command(tab)

    assert cancelled == [True]


def test_close_command_closes_the_last_answer_tab(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    tabbed = FakeTabbedBrowser()
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)
    target = FakeAnswerTab()
    plugin._last_answer_tab = target

    plugin._close_command(FakeTab())

    assert tabbed.closed == [target]


def test_close_command_skips_a_tab_that_is_being_removed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    infos: list[str] = []
    monkeypatch.setattr(plugin.message, "info", infos.append)
    monkeypatch.setattr(
        plugin.objreg, "get", lambda *args, **kwargs: pytest.fail("looked up")
    )
    target = FakeAnswerTab()
    target.pending_removal = True
    plugin._last_answer_tab = target

    plugin._close_command(FakeTab())

    assert infos == ["docent: no answer tab"]


def test_close_command_reports_when_no_answer_tab_exists(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    infos: list[str] = []
    monkeypatch.setattr(plugin.message, "info", infos.append)

    plugin._close_command(FakeTab())

    assert infos == ["docent: no answer tab"]


def test_close_command_survives_a_missing_tabbed_browser(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def fail(*args: Any, **kwargs: Any) -> None:
        raise KeyError("tabbed-browser")

    monkeypatch.setattr(plugin.objreg, "get", fail)
    plugin._last_answer_tab = FakeAnswerTab()

    plugin._close_command(FakeTab())


def test_pick_command_reports_profile_errors(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def fail(force: bool = False) -> list[ProfileInfo]:
        raise ProfileError("no profiles")

    monkeypatch.setattr(plugin, "profiles", fail)
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)

    plugin._pick_command(FakeTab())

    assert errors == ["docent: no profiles"]


def test_pick_command_reports_an_empty_catalog(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [])
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)

    plugin._pick_command(FakeTab())

    assert errors == ["docent: no profiles found; run 'docent init'"]


def test_pick_command_does_nothing_when_the_dialog_is_cancelled(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profiles = [ProfileInfo("explain", "Explain a passage", ("selection",))]
    monkeypatch.setattr(plugin, "profiles", lambda force=False: profiles)
    plugin._settings = _settings()
    monkeypatch.setattr(
        plugin.QInputDialog, "getItem", lambda *args, **kwargs: ("explain", False)
    )
    monkeypatch.setattr(plugin, "_run_request", lambda *args: pytest.fail("ran"))

    plugin._pick_command(FakeTab())


def test_open_answer_reports_a_missing_tabbed_browser(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)

    def fail(*args: Any, **kwargs: Any) -> None:
        raise KeyError("tabbed-browser")

    monkeypatch.setattr(plugin.objreg, "get", fail)

    plugin._open_answer(FakeTab(), _profile(), {"selection": "x"}, None, None)

    assert errors and errors[0].startswith("docent: could not open an answer tab:")


def test_open_answer_reports_a_failed_tab_open(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)
    tabbed = FakeTabbedBrowser(tabopen_error=RuntimeError("no window"))
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)

    plugin._open_answer(FakeTab(), _profile(), {}, None, None)

    assert errors == ["docent: could not open an answer tab: no window"]


def test_open_answer_opens_a_tab_and_starts_the_session(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    started: list[tuple[dict[str, Any], str | None, str | None]] = []
    monkeypatch.setattr(
        plugin.Session,
        "start",
        lambda self, request, target, context: started.append(
            (request, target, context)
        ),
    )
    tabbed = FakeTabbedBrowser()
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)

    plugin._open_answer(FakeTab(), _profile(), {"selection": "x"}, "French", "full")

    answer_tab = tabbed.answer_tab
    session = plugin._sessions[answer_tab.tab_id]
    assert tabbed.opened == [False]
    assert answer_tab.html and "explain" in answer_tab.html[0]
    assert plugin._last_answer_tab is answer_tab
    assert started == []

    answer_tab.load_finished.emit(True)

    assert started == [({"selection": "x"}, "French", "full")]
    assert session.started
    assert session.on_navigation in answer_tab.load_started.handlers


def test_open_answer_reports_a_page_that_does_not_load(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)
    tabbed = FakeTabbedBrowser()
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)
    plugin._open_answer(FakeTab(), _profile(), {}, None, None)
    session = plugin._sessions[tabbed.answer_tab.tab_id]

    tabbed.answer_tab.load_finished.emit(False)

    assert session.page.failed
    assert errors == ["docent: could not load the answer page"]


def test_open_answer_reports_a_shell_that_cannot_be_set(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)
    tabbed = FakeTabbedBrowser(answer_tab=BrokenAnswerTab())
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)

    plugin._open_answer(FakeTab(), _profile(), {}, None, None)

    session = plugin._sessions[tabbed.answer_tab.tab_id]
    assert session.page.failed
    assert errors == ["docent: could not open the answer tab: shell"]


def test_package_setup_configures_the_plugin() -> None:
    config = FakeConfig()

    docent.setup(config, cmd=["docent", "--debug"])

    assert plugin._settings is not None
    assert plugin._settings.config is config
    assert plugin._settings.cmd == ["docent", "--debug"]


def test_package_refresh_clears_the_cached_profiles() -> None:
    plugin._profiles = [ProfileInfo("explain", None, ())]

    docent.refresh()

    assert plugin._profiles is None


def test_package_profile_list_loads_from_the_plugin(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profiles = [ProfileInfo("explain", None, ("selection",))]
    monkeypatch.setattr(plugin, "load_profiles", lambda cmd, config_dir=None: profiles)
    plugin._settings = _settings()

    assert docent.profile_list(force=True) == profiles
    assert plugin._profiles == profiles


def test_package_stylesheet_returns_the_ui_stylesheet() -> None:
    assert docent.stylesheet() == ui.stylesheet()
    assert "--docent-bg" in docent.stylesheet()


class StubbornSignal(FakeSignal):
    """Signal that refuses to drop a handler twice."""

    def disconnect(self, handler: Any) -> None:
        raise RuntimeError("already disconnected")


class StubbornAnswerTab(FakeAnswerTab):
    """Answer tab whose loader reports a second disconnect."""

    def __init__(self) -> None:
        super().__init__()
        self.load_finished = StubbornSignal()


class BrokenCaptureTab(FakeTab):
    """Tab that cannot inject JavaScript."""

    def run_js_async(self, code: str, callback: Any = None) -> None:
        raise RuntimeError("no js")


class VanishedTab:
    """Answer tab whose state raises once qutebrowser dropped it."""

    win_id = 3

    @property
    def pending_removal(self) -> bool:
        raise RuntimeError("closed")


def test_session_finishes_after_cancel_without_touching_the_page(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    _install_fake_process(monkeypatch)
    page = FakePage()
    session = _session(page)
    process = plugin.QProcess()
    session.process = process
    session.cancelled = True

    session._finished(0, None)  # type: ignore[arg-type]

    assert process.deleted
    assert page.finished == 0
    assert page.errors == []


def test_session_reports_a_truncated_final_event() -> None:
    page = FakePage()
    session = _session(page)
    session.parser.feed(b'{"type"')

    session._finished(0, None)  # type: ignore[arg-type]

    assert session.error
    assert page.errors and page.errors[0].startswith("invalid stream event:")
    assert page.finished == 0


def test_profiles_loads_once_until_forced(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    calls: list[list[str]] = []
    catalog = [ProfileInfo("explain", None, ())]

    def load(cmd: list[str], config_dir: str | None = None) -> list[ProfileInfo]:
        calls.append(cmd)
        return catalog

    monkeypatch.setattr(plugin, "load_profiles", load)
    plugin._settings = _settings()

    assert plugin.profiles() == catalog
    assert plugin.profiles() == catalog
    assert len(calls) == 1

    assert plugin.profiles(force=True) == catalog
    assert len(calls) == 2


def test_profiles_requires_setup() -> None:
    with pytest.raises(plugin.DocentError, match="docent.setup"):
        plugin.profiles()


def test_refresh_command_clears_the_cache(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    infos: list[str] = []
    monkeypatch.setattr(plugin.message, "info", infos.append)
    plugin._profiles = [ProfileInfo("explain", None, ())]

    plugin._refresh_command()

    assert plugin._profiles is None
    assert infos == ["docent: profiles refreshed"]


def test_docent_command_runs_the_named_profile(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[tuple[Any, str, dict[str, Any]]] = []
    monkeypatch.setattr(
        plugin,
        "_run_request",
        lambda tab, name, **kwargs: ran.append((tab, name, kwargs)),
    )
    tab = FakeTab()

    plugin._docent_command(
        tab,
        "ask",
        question="Why?",
        target_language="French",
        context_mode="full",
    )

    assert ran == [
        (
            tab,
            "ask",
            {
                "question": "Why?",
                "target_language": "French",
                "context_mode": "full",
            },
        )
    ]


def test_run_request_stops_when_the_tab_is_being_removed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile = ProfileInfo("explain", None, ("selection",))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    monkeypatch.setattr(plugin, "_open_answer", lambda *args: pytest.fail("opened"))
    tab = CaptureTab({"selection": "passage", "document": "page"})
    tab.pending_removal = True

    plugin._run_request(
        tab,
        "explain",
        question=None,
        target_language=None,
        context_mode=None,
    )


def test_run_request_reports_an_unreadable_page(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile = ProfileInfo("explain", None, ("selection",))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)
    monkeypatch.setattr(plugin, "_open_answer", lambda *args: pytest.fail("opened"))

    plugin._run_request(
        CaptureTab("not a capture"),
        "explain",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert errors == ["docent: could not read this page"]


def test_run_request_reports_a_page_without_text(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile = ProfileInfo("summarize", None, ("context",))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)
    monkeypatch.setattr(plugin, "_open_answer", lambda *args: pytest.fail("opened"))

    plugin._run_request(
        CaptureTab({"selection": "", "document": "   "}),
        "summarize",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert errors == ["docent: the page has no text to send"]


def test_run_request_reports_a_tab_that_cannot_run_javascript(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    profile = ProfileInfo("explain", None, ("selection",))
    monkeypatch.setattr(plugin, "profiles", lambda force=False: [profile])
    plugin._settings = _settings()
    errors: list[str] = []
    monkeypatch.setattr(plugin.message, "error", errors.append)

    plugin._run_request(
        BrokenCaptureTab(),
        "explain",
        question=None,
        target_language=None,
        context_mode=None,
    )

    assert errors == ["docent: could not read this page: no js"]


def test_open_answer_skips_a_cancelled_session(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    monkeypatch.setattr(plugin.Session, "start", lambda *args: pytest.fail("started"))
    tabbed = FakeTabbedBrowser()
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)

    plugin._open_answer(FakeTab(), _profile(), {}, None, None)
    session = plugin._sessions[tabbed.answer_tab.tab_id]
    session.cancelled = True

    tabbed.answer_tab.load_finished.emit(True)


def test_open_answer_ignores_a_stale_loader_connection(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    plugin._settings = _settings()
    started: list[bool] = []
    monkeypatch.setattr(
        plugin.Session,
        "start",
        lambda *args: started.append(True),
    )
    tabbed = FakeTabbedBrowser(answer_tab=StubbornAnswerTab())
    monkeypatch.setattr(plugin.objreg, "get", lambda *args, **kwargs: tabbed)

    plugin._open_answer(FakeTab(), _profile(), {}, None, None)

    tabbed.answer_tab.load_finished.emit(True)

    assert started == [True]


def test_close_command_ignores_a_vanished_answer_tab(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    infos: list[str] = []
    monkeypatch.setattr(plugin.message, "info", infos.append)
    plugin._last_answer_tab = VanishedTab()  # type: ignore[assignment]

    plugin._close_command(FakeTab())

    assert infos == ["docent: no answer tab"]
