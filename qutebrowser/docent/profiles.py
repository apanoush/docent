"""Read profile metadata from the docent CLI."""

from __future__ import annotations

import json
import subprocess
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from typing import Any

__all__ = ["ProfileError", "ProfileInfo", "has_requirement", "load_profiles"]


class ProfileError(Exception):
    """Raised when profile metadata cannot be loaded."""


@dataclass(frozen=True, slots=True)
class ProfileInfo:
    """Metadata for one docent profile."""

    name: str
    description: str | None
    requires: tuple[str, ...]


def has_requirement(profile: ProfileInfo, name: str) -> bool:
    """Return whether a profile declares the given input requirement."""
    return name in profile.requires


def load_profiles(
    cmd: Sequence[str],
    *,
    config_dir: str | None = None,
    env: Mapping[str, str] | None = None,
    timeout: float = 5.0,
) -> list[ProfileInfo]:
    """Run ``docent profiles --json`` and return the parsed catalog.

    The command is local and does not contact a provider, so a short blocking
    call is acceptable at command start and in the profile picker.
    """
    if not cmd:
        raise ProfileError("no docent command configured")

    args = list(cmd)
    if config_dir:
        args.extend(["--config-dir", config_dir])
    args.extend(["profiles", "--json"])

    try:
        result = subprocess.run(
            args,
            capture_output=True,
            text=True,
            encoding="utf-8",
            env=env,
            timeout=timeout,
            check=False,
        )
    except FileNotFoundError as exc:
        raise ProfileError(f"could not run {args[0]}: {exc}") from exc
    except subprocess.TimeoutExpired as exc:
        raise ProfileError(f"{args[0]} profiles timed out") from exc
    except OSError as exc:
        raise ProfileError(f"could not run {args[0]}: {exc}") from exc

    if result.returncode != 0:
        message = result.stderr.strip()
        if not message:
            message = f"{args[0]} exited with code {result.returncode}"
        raise ProfileError(message)

    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise ProfileError(f"failed to parse profile list: {exc}") from exc

    if not isinstance(payload, dict):
        raise ProfileError("failed to parse profile list: expected an object")
    entries = payload.get("profiles")
    if not isinstance(entries, list):
        raise ProfileError("failed to parse profile list: missing profiles array")

    profiles: list[ProfileInfo] = []
    for entry in entries:
        profiles.append(_parse_profile(entry))
    return profiles


def _parse_profile(entry: Any) -> ProfileInfo:
    """Validate one entry from the CLI's JSON output."""
    if not isinstance(entry, dict) or not isinstance(entry.get("name"), str):
        raise ProfileError("failed to parse profile list: invalid profile entry")

    name = entry["name"]
    description = entry.get("description")
    if description is not None and not isinstance(description, str):
        raise ProfileError(f"failed to parse profile {name}: invalid description")

    requires = entry.get("requires") or []
    if not isinstance(requires, list) or not all(
        isinstance(item, str) for item in requires
    ):
        raise ProfileError(f"failed to parse profile {name}: invalid requires")

    return ProfileInfo(
        name=name,
        description=description,
        requires=tuple(requires),
    )
