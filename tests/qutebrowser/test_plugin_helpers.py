"""Tests for qutebrowser-facing helpers that do not require a browser."""

from __future__ import annotations

from typing import Any

import pytest
from docent import plugin, ui
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
        self.finished = 0
        self.closed = False

    def append_text(self, text: str) -> None:
        self.text.append(text)

    def set_error(self, message: str) -> None:
        self.errors.append(message)

    def done(self, model: str | None = None) -> None:
        self.models.append(model)

    def finish(self) -> None:
        self.finished += 1

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


def test_answer_page_finish_reports_no_output() -> None:
    tab = FakeTab()
    page = ui.AnswerPage(tab, "explain")

    page.finish()

    assert page.finished
    assert "(no output)" in tab.codes[0]


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
    session._handle_event({"type": "done", "model": "test-model"})
    session._handle_event({"type": "error", "message": "boom"})
    session._handle_event({"type": "unknown"})

    assert page.text == ["hello"]
    assert page.models == ["test-model"]
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
    assert page.errors == []


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


def test_run_arguments_tags_the_source() -> None:
    args = plugin.run_arguments(_settings(), "explain", "French", "full")

    assert args == [
        "run",
        "--profile",
        "explain",
        "--stream",
        "--target-language",
        "French",
        "--context-mode",
        "full",
        "--source",
        "qutebrowser",
    ]


def test_run_arguments_include_config_dir() -> None:
    settings = _settings()
    settings.config_dir = "/tmp/docent"

    args = plugin.run_arguments(settings, "ask", None, None)

    assert args == [
        "--config-dir",
        "/tmp/docent",
        "run",
        "--profile",
        "ask",
        "--stream",
        "--source",
        "qutebrowser",
    ]
