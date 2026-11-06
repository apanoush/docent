"""Tests for the docent profile loader."""

from __future__ import annotations

import sys
from textwrap import dedent

import pytest
from docent.profiles import ProfileError, has_requirement, load_profiles


def _cmd(source: str) -> list[str]:
    return [sys.executable, "-c", dedent(source)]


def test_valid_catalog_is_parsed() -> None:
    source = """
        import json
        print(json.dumps({"profiles": [
            {"name": "explain", "description": "Explain a passage",
             "requires": ["selection"]},
            {"name": "ask", "requires": ["selection", "question"]},
        ]}))
    """

    profiles = load_profiles(_cmd(source))

    assert [profile.name for profile in profiles] == ["explain", "ask"]
    assert profiles[0].description == "Explain a passage"
    assert profiles[0].requires == ("selection",)
    assert profiles[1].description is None
    assert profiles[1].requires == ("selection", "question")


def test_config_dir_is_passed_before_the_subcommand() -> None:
    source = """
        import json
        import sys
        assert sys.argv[1:] == ["--config-dir", "/tmp/example", "profiles", "--json"], sys.argv
        print(json.dumps({"profiles": []}))
    """

    profiles = load_profiles(_cmd(source), config_dir="/tmp/example")

    assert profiles == []


def test_nonzero_exit_reports_stderr() -> None:
    source = """
        import sys
        print("provider unavailable", file=sys.stderr)
        raise SystemExit(3)
    """

    with pytest.raises(ProfileError, match="provider unavailable"):
        load_profiles(_cmd(source))


def test_nonzero_exit_without_stderr_reports_code() -> None:
    source = "raise SystemExit(3)"

    with pytest.raises(ProfileError, match="exited with code 3"):
        load_profiles(_cmd(source))


def test_invalid_json_raises() -> None:
    source = "print('not json')"

    with pytest.raises(ProfileError, match="failed to parse profile list"):
        load_profiles(_cmd(source))


def test_missing_profiles_array_raises() -> None:
    source = "import json; print(json.dumps({}))"

    with pytest.raises(ProfileError, match="missing profiles array"):
        load_profiles(_cmd(source))


def test_invalid_profile_entry_raises() -> None:
    source = 'import json; print(json.dumps({"profiles": [{"description": "x"}]}))'

    with pytest.raises(ProfileError, match="invalid profile entry"):
        load_profiles(_cmd(source))


def test_invalid_description_raises() -> None:
    source = 'import json; print(json.dumps({"profiles": [{"name": "x", "description": 3}]}))'

    with pytest.raises(ProfileError, match="invalid description"):
        load_profiles(_cmd(source))


def test_invalid_requires_raises() -> None:
    source = (
        'import json; print(json.dumps({"profiles": [{"name": "x", "requires": [3]}]}))'
    )

    with pytest.raises(ProfileError, match="invalid requires"):
        load_profiles(_cmd(source))


def test_empty_requires_becomes_a_tuple() -> None:
    source = 'import json; print(json.dumps({"profiles": [{"name": "x"}]}))'

    profiles = load_profiles(_cmd(source))

    assert profiles[0].requires == ()


def test_missing_binary_raises() -> None:
    with pytest.raises(ProfileError, match="could not run"):
        load_profiles(["/nonexistent/docent-for-test"])


def test_empty_command_raises() -> None:
    with pytest.raises(ProfileError, match="no docent command"):
        load_profiles([])


def test_has_requirement() -> None:
    source = 'import json; print(json.dumps({"profiles": [{"name": "x", "requires": ["selection"]}]}))'
    profile = load_profiles(_cmd(source))[0]

    assert has_requirement(profile, "selection")
    assert not has_requirement(profile, "question")
