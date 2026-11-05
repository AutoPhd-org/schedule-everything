"""
AI conference dates for the browser's year calendar.

Submission deadlines and conference dates for 15 major AI conferences come from
the community-maintained Hugging Face AI Deadlines data
(https://github.com/huggingface/ai-deadlines). Data is fetched only when the
user turns the conference layer on, then kept in a small normalized cache file
that is refreshed at most once a week or on request; nothing stays in memory.
"""

from __future__ import annotations

import json
import os
import re
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta, timezone, tzinfo
from pathlib import Path
from typing import Any
from urllib.request import Request, urlopen
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

import yaml

from schedule_management.config_layout import resolve_config_root_dir

SOURCE_URL = "https://github.com/huggingface/ai-deadlines"
RAW_URL = "https://raw.githubusercontent.com/huggingface/ai-deadlines/main/src/data/conferences/{id}.yml"
CACHE_TTL = timedelta(days=7)
CACHE_VERSION = 1  # bump when normalize_editions changes, since the cache stores its output
FETCH_TIMEOUT = 10

# (id, display name, group); groups drive the calendar colors.
CONFERENCES: tuple[tuple[str, str, str], ...] = (
    ("neurips", "NeurIPS", "ml"), ("iclr", "ICLR", "ml"), ("icml", "ICML", "ml"),
    ("cvpr", "CVPR", "vision"), ("iccv", "ICCV", "vision"), ("eccv", "ECCV", "vision"),
    ("acl", "ACL", "language"), ("emnlp", "EMNLP", "language"), ("colm", "COLM", "language"),
    ("aaai", "AAAI", "ai"), ("ijcai", "IJCAI", "ai"),
    ("icra", "ICRA", "robotics"), ("iros", "IROS", "robotics"), ("rss", "RSS", "robotics"), ("corl", "CoRL", "robotics"),
)
CONFERENCE_IDS = tuple(conference_id for conference_id, _, _ in CONFERENCES)

# Deadline types shown on the calendar: abstract registration and paper submission.
_ABSTRACT_TYPES = {"abstract", "registration"}
_SUBMISSION_TYPES = {"paper", "submission", "full_paper"}
# The generic types also cover side tracks (e.g. "Workshop Proposal Submission", "Paper video submission deadline").
_SIDE_TRACK = re.compile(r"\b(workshops?|tutorials?|proposals?|art|videos?|demos?|demonstrations?|supplement\w*|challenges?|"
                         r"competitions?|doctoral|industry|camera|rebuttal|journal|posters?|late[- ]breaking)\b", re.IGNORECASE)
_FIXED_ZONES = {"aoe": -12, "utc": 0, "gmt": 0, "z": 0, "pst": -8, "pdt": -7, "mst": -7, "mdt": -6, "cst": -6, "cdt": -5,
                "est": -5, "edt": -4, "cet": 1, "cest": 2, "bst": 1, "ist": 5.5, "jst": 9, "kst": 9, "sgt": 8, "aest": 10}
_MONTHS = {name: index for index, name in enumerate(
    ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"], start=1)}


def _cache_path() -> Path:
    override = os.getenv("REMINDER_CONFERENCE_CACHE_PATH")
    if override:
        return Path(override).expanduser()
    return resolve_config_root_dir() / "cache" / "ai_conferences.json"


def _local_timezone() -> tzinfo | None:
    """None means the system time zone, applied per date so daylight saving time is respected."""
    return None


def _zone(name: Any) -> tzinfo:
    """Resolve a deadline time zone (AoE, UTC-8, PST, or an IANA name); unknown zones count as AoE."""
    text = str(name or "AoE").strip()
    fixed = _FIXED_ZONES.get(text.lower())
    if fixed is not None:
        return timezone(timedelta(hours=fixed))
    match = re.fullmatch(r"(?:UTC|GMT)\s*([+-])\s*(\d{1,2})(?::?(\d{2}))?", text, re.IGNORECASE)
    if match:
        sign = 1 if match.group(1) == "+" else -1
        return timezone(sign * timedelta(hours=int(match.group(2)), minutes=int(match.group(3) or 0)))
    try:
        return ZoneInfo(text)
    except (ZoneInfoNotFoundError, ValueError):
        return timezone(timedelta(hours=-12))


def _instant(value: Any, zone_name: Any) -> datetime | None:
    """Parse a deadline value into an aware UTC datetime."""
    if isinstance(value, datetime):
        moment = value
    elif isinstance(value, date):
        moment = datetime(value.year, value.month, value.day, 23, 59, 59)
    else:
        try:
            moment = datetime.fromisoformat(str(value).strip())
        except ValueError:
            return None
    if moment.tzinfo is None:
        moment = moment.replace(tzinfo=_zone(zone_name))
    return moment.astimezone(timezone.utc)


def _iso_date(value: Any) -> str | None:
    if isinstance(value, datetime):
        return value.date().isoformat()
    if isinstance(value, date):
        return value.isoformat()
    try:
        return date.fromisoformat(str(value).strip()[:10]).isoformat()
    except ValueError:
        return None


def _range_from_text(text: Any, year: Any) -> tuple[str | None, str | None]:
    """Parse "June 3-7, 2026" or "September 27 - October 1, 2026" into ISO start and end."""
    match = re.fullmatch(r"\s*([A-Za-z]+)\s+(\d{1,2})\s*(?:-|–)\s*(?:([A-Za-z]+)\s+)?(\d{1,2}),?\s*(\d{4})?\s*", str(text or ""))
    if not match:
        return None, None
    first_month = _MONTHS.get(match.group(1).lower())
    last_month = _MONTHS.get((match.group(3) or match.group(1)).lower())
    try:
        end_year = int(match.group(5) or year)
        start_year = end_year - 1 if first_month and last_month and first_month > last_month else end_year
        return (date(start_year, first_month, int(match.group(2))).isoformat(),
                date(end_year, last_month, int(match.group(4))).isoformat()) if first_month and last_month else (None, None)
    except (TypeError, ValueError):
        return None, None


def _safe_link(value: Any) -> str | None:
    link = str(value or "").strip()
    return link if link.startswith(("https://", "http://")) else None


def normalize_editions(text: str) -> list[dict[str, Any]]:
    """Turn one conference's YAML file into compact editions with UTC deadline instants."""
    data = yaml.safe_load(text) or []
    if not isinstance(data, list):
        raise ValueError("conference data must be a list of editions")
    editions: list[dict[str, Any]] = []
    for entry in data:
        if not isinstance(entry, dict) or not isinstance(entry.get("year"), int):
            continue
        raw_deadlines = entry.get("deadlines") if isinstance(entry.get("deadlines"), list) else []
        if not raw_deadlines:
            # Older editions use flat fields with one shared time zone.
            raw_deadlines = [{"type": "abstract", "label": "Abstract deadline", "date": entry.get("abstract_deadline"), "timezone": entry.get("timezone")},
                             {"type": "paper", "label": "Paper deadline", "date": entry.get("deadline"), "timezone": entry.get("timezone")}]
        deadlines = []
        for item in raw_deadlines:
            if not isinstance(item, dict) or item.get("date") in (None, ""):
                continue
            kind = str(item.get("type") or "").lower()
            label = str(item.get("label") or "")
            if kind not in _ABSTRACT_TYPES | _SUBMISSION_TYPES or _SIDE_TRACK.search(label):
                continue
            # "Registration" can also mean attendee registration; only paper or abstract registration counts.
            if kind == "registration" and not re.search(r"\b(paper|abstract)\b", label, re.IGNORECASE):
                continue
            moment = _instant(item.get("date"), item.get("timezone"))
            if moment is None:
                continue
            deadlines.append({
                "kind": "abstract" if kind in _ABSTRACT_TYPES else "submission",
                "label": (label or ("Abstract deadline" if kind in _ABSTRACT_TYPES else "Paper deadline"))[:120],
                "utc": moment.isoformat(),
                "timezone": str(item.get("timezone") or "AoE")[:40],
            })
        start, end = _iso_date(entry.get("start")), _iso_date(entry.get("end"))
        if not start or not end:
            start, end = _range_from_text(entry.get("date"), entry.get("year"))
        place = ", ".join(str(part) for part in (entry.get("city"), entry.get("country")) if part)
        editions.append({
            "year": entry["year"],
            "fullName": str(entry.get("full_name") or "")[:200],
            "link": _safe_link(entry.get("link")),
            "place": place[:120],
            "dateText": str(entry.get("date") or "")[:80],
            "start": start,
            "end": end,
            "deadlines": deadlines,
        })
    return editions


def _fetch_text(url: str) -> str:
    request = Request(url, headers={"User-Agent": "schedule-everything"})
    with urlopen(request, timeout=FETCH_TIMEOUT) as response:  # noqa: S310 - fixed https URL
        return response.read(1_000_000).decode("utf-8")


def _read_cache() -> dict[str, Any] | None:
    try:
        cache = json.loads(_cache_path().read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None
    return cache if isinstance(cache, dict) and cache.get("version") == CACHE_VERSION else None


def _refresh(previous: dict[str, Any] | None, now: datetime) -> dict[str, Any]:
    """Fetch every conference; keep the previous data for any conference that fails."""
    def fetch(conference_id: str) -> tuple[str, list[dict[str, Any]] | None, str | None]:
        try:
            return conference_id, normalize_editions(_fetch_text(RAW_URL.format(id=conference_id))), None
        except Exception as exc:  # network, HTTP, or parse failure for one conference
            return conference_id, None, str(exc)[:200]

    with ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(fetch, CONFERENCE_IDS))
    old = (previous or {}).get("conferences", {})
    conferences = {conference_id: editions if editions is not None else old.get(conference_id, []) for conference_id, editions, _ in results}
    errors = {conference_id: error for conference_id, _, error in results if error}
    if len(errors) == len(CONFERENCE_IDS):
        raise ConnectionError(next(iter(errors.values())))
    cache = {"version": CACHE_VERSION, "source": SOURCE_URL, "fetchedAt": now.isoformat(), "conferences": conferences, "errors": errors}
    path = _cache_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(cache, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    temporary.replace(path)
    return cache


def load_conference_data(refresh: bool = False, now: datetime | None = None) -> tuple[dict[str, Any] | None, str | None]:
    """Return (cache, error). Fetch only when forced, missing, or older than a week."""
    moment = now or datetime.now(timezone.utc)
    cache = _read_cache()
    fresh = False
    if cache:
        try:
            fresh = moment - datetime.fromisoformat(cache["fetchedAt"]) < CACHE_TTL
        except (KeyError, TypeError, ValueError):
            fresh = False
    if cache and fresh and not refresh:
        return cache, None
    try:
        return _refresh(cache, moment), None
    except Exception as exc:
        message = f"Could not fetch conference dates: {exc}"
        return cache, message[:240]


def calendar_events(cache: dict[str, Any], year: int, local_zone: tzinfo | None = None) -> list[dict[str, Any]]:
    """Deadlines and conference dates that fall in `year`, with deadlines in local time."""
    zone = local_zone if local_zone is not None else _local_timezone()
    names = {conference_id: (name, group) for conference_id, name, group in CONFERENCES}
    events: list[dict[str, Any]] = []
    for conference_id, editions in cache.get("conferences", {}).items():
        if conference_id not in names:
            continue
        name, group = names[conference_id]
        for edition in editions:
            common = {"conference": conference_id, "name": name, "group": group, "edition": edition["year"],
                      "fullName": edition.get("fullName"), "link": edition.get("link"), "place": edition.get("place")}
            for index, deadline in enumerate(edition.get("deadlines", [])):
                local = datetime.fromisoformat(deadline["utc"]).astimezone(zone)
                if local.year != year:
                    continue
                source = datetime.fromisoformat(deadline["utc"]).astimezone(_zone(deadline["timezone"]))
                events.append({**common, "id": f"{conference_id}{edition['year']}-{deadline['kind']}-{index}", "kind": deadline["kind"],
                               "label": deadline["label"], "date": local.date().isoformat(), "endDate": None,
                               "time": local.strftime("%H:%M"), "sourceTime": f"{source:%b} {source.day}, {source:%H:%M} {deadline['timezone']}"})
            start, end = edition.get("start"), edition.get("end")
            if start and end and start[:4] <= str(year) <= end[:4]:
                events.append({**common, "id": f"{conference_id}{edition['year']}-conference", "kind": "conference",
                               "label": f"{name} {edition['year']}", "date": start, "endDate": end, "time": None,
                               "sourceTime": edition.get("dateText") or None})
    return sorted(events, key=lambda event: (event["date"], event["name"], event["kind"]))


def conference_calendar(payload: dict[str, Any]) -> dict[str, Any]:
    """Browser API: AI conference events for one calendar year."""
    from schedule_management.web.services import WebError

    try:
        year = int(payload.get("year"))
    except (TypeError, ValueError):
        raise WebError("invalid_input", "year must be an integer.") from None
    if not 1970 <= year <= 2200:
        raise WebError("invalid_input", "year is out of range.")
    cache, error = load_conference_data(refresh=payload.get("refresh") is True)
    return {
        "year": year,
        "source": SOURCE_URL,
        "fetchedAt": cache.get("fetchedAt") if cache else None,
        "error": error,
        "conferences": [{"id": conference_id, "name": name, "group": group} for conference_id, name, group in CONFERENCES],
        "events": calendar_events(cache, year) if cache else [],
    }
