"""AI conference dates: parsing, time zones, caching, and the browser API (no network)."""

from __future__ import annotations

import json
from datetime import datetime, timedelta, timezone

import pytest

NEURIPS = """
- title: NeurIPS
  year: 2026
  id: neurips26
  full_name: Conference on Neural Information Processing Systems
  link: https://neurips.cc/
  deadlines:
    - type: abstract
      label: Abstract registration deadline
      date: '2026-05-04 23:59:59'
      timezone: AoE
    - type: paper
      label: Paper submission deadline
      date: '2026-05-06 23:59:59'
      timezone: AoE
    - type: notification
      label: Author notification
      date: '2026-09-24 23:59:59'
      timezone: AoE
    - type: submission
      label: Workshop Proposal Submission
      date: '2026-04-10 23:59:59'
      timezone: AoE
    - type: paper
      label: Paper video submission deadline
      date: '2026-05-10 23:59:59'
      timezone: AoE
    - type: registration
      label: Early registration
      date: '2026-10-01 23:59:59'
      timezone: AoE
  city: Sydney
  country: Australia
  date: December 6-12, 2026
  start: '2026-12-06'
  end: '2026-12-12'
"""
CVPR = """
- title: CVPR
  year: 2025
  id: cvpr25
  link: javascript:alert(1)
  deadline: '2024-11-14 23:59:00'
  abstract_deadline: '2024-11-07 23:59:00'
  timezone: UTC-8
  date: June 10-17, 2025
- title: CVPR
  year: 2026
  id: cvpr26
  link: https://cvpr.thecvf.com/
  deadlines:
  - type: registration
    label: Paper Registration
    date: '2025-11-06 23:59:59'
    timezone: AoE
  - type: paper
    label: Paper Submission
    date: '2025-11-13 23:59:59'
    timezone: AoE
  date: June 3-7, 2026
  start: 2026-06-03
  end: 2026-06-07
"""
IROS = """
- title: IROS
  year: 2026
  id: iros26
  link: https://www.iros2026.org/
  deadlines:
  - type: submission
    label: Paper Submission
    date: '2026-03-01 23:59:59'
    timezone: PST
  date: September 27 - October 1, 2026
"""
PACIFIC = timezone(timedelta(hours=-8))


@pytest.fixture
def conferences(monkeypatch, tmp_path):
    from schedule_management import conferences as module

    monkeypatch.setenv("REMINDER_CONFERENCE_CACHE_PATH", str(tmp_path / "cache" / "ai_conferences.json"))
    monkeypatch.setattr(module, "_local_timezone", lambda: PACIFIC)
    files = {"neurips": NEURIPS, "cvpr": CVPR, "iros": IROS}
    calls: list[str] = []

    def fake_fetch(url: str) -> str:
        conference_id = url.rsplit("/", 1)[1].removesuffix(".yml")
        calls.append(conference_id)
        if conference_id not in files:
            raise OSError("404 Not Found")
        return files[conference_id]

    monkeypatch.setattr(module, "_fetch_text", fake_fetch)
    module.calls = calls
    return module


def test_normalizes_deadline_lists_legacy_fields_and_date_text(conferences):
    editions = conferences.normalize_editions(CVPR)
    legacy, current = editions
    assert [d["kind"] for d in legacy["deadlines"]] == ["abstract", "submission"]
    assert legacy["deadlines"][1]["utc"] == "2024-11-15T07:59:00+00:00"  # UTC-8
    assert legacy["link"] is None, "only http(s) links are kept"
    assert (legacy["start"], legacy["end"]) == ("2025-06-10", "2025-06-17")  # parsed from the date text
    assert (current["start"], current["end"]) == ("2026-06-03", "2026-06-07")  # bare YAML dates
    iros = conferences.normalize_editions(IROS)[0]
    assert (iros["start"], iros["end"]) == ("2026-09-27", "2026-10-01")
    assert iros["deadlines"][0]["utc"] == "2026-03-02T07:59:59+00:00"  # PST
    neurips = conferences.normalize_editions(NEURIPS)[0]
    # Notifications, side tracks (workshops, videos), and attendee registration are left out.
    assert [d["label"] for d in neurips["deadlines"]] == ["Abstract registration deadline", "Paper submission deadline"]
    assert [(d["kind"], d["label"]) for d in current["deadlines"]] == [("abstract", "Paper Registration"), ("submission", "Paper Submission")]
    assert neurips["place"] == "Sydney, Australia"


@pytest.mark.parametrize(("zone", "offset"), [("AoE", -12), ("UTC", 0), ("UTC+5:30", 5.5), ("GMT-3", -3), ("PST", -8), ("Mars/Olympus", -12)])
def test_time_zone_names(conferences, zone, offset):
    moment = datetime(2026, 1, 1, 12, tzinfo=timezone.utc)
    assert conferences._zone(zone).utcoffset(moment) == timedelta(hours=offset)
    assert conferences._zone("Europe/Vienna").utcoffset(moment) == timedelta(hours=1)


def test_calendar_api_returns_year_events_in_local_time(conferences):
    from schedule_management.web.api import dispatch

    response = dispatch({"command": "conference_calendar", "payload": {"year": 2026}})
    assert response["ok"] is True
    data = response["data"]
    assert data["error"] is None
    assert [c["id"] for c in data["conferences"]][:3] == ["neurips", "iclr", "icml"]
    events = {event["id"]: event for event in data["events"]}
    paper = events["neurips2026-submission-1"]
    # 23:59 AoE on May 6 is 03:59 on May 7 in Pacific time.
    assert (paper["date"], paper["time"], paper["sourceTime"]) == ("2026-05-07", "03:59", "May 6, 23:59 AoE")
    assert (paper["kind"], paper["group"], paper["name"]) == ("submission", "ml", "NeurIPS")
    conference = events["neurips2026-conference"]
    assert (conference["date"], conference["endDate"], conference["place"]) == ("2026-12-06", "2026-12-12", "Sydney, Australia")
    assert "neurips2026-notification" not in str(events), "only submission dates and conference dates are shown"
    assert not [i for i in events if i.startswith("cvpr2026-") and not i.endswith("-conference")], "CVPR 2026 deadlines fell in 2025"
    assert "cvpr2026-conference" in events and "iros2026-conference" in events
    assert [e["date"] for e in data["events"]] == sorted(e["date"] for e in data["events"])

    assert dispatch({"command": "conference_calendar", "payload": {"year": "soon"}})["error"]["code"] == "invalid_input"


def test_local_times_follow_daylight_saving_time(conferences, monkeypatch):
    import time

    monkeypatch.setattr(conferences, "_local_timezone", lambda: None)  # the real default: the system zone
    monkeypatch.setenv("TZ", "America/Los_Angeles")
    time.tzset()
    try:
        cache = {"conferences": {"neurips": conferences.normalize_editions(NEURIPS)}}
        events = {event["id"]: event for event in conferences.calendar_events(cache, 2026)}
    finally:
        monkeypatch.undo()
        time.tzset()
    # 23:59 AoE on May 6 is 04:59 in Los Angeles in May (PDT), not 03:59 (PST).
    assert (events["neurips2026-submission-1"]["date"], events["neurips2026-submission-1"]["time"]) == ("2026-05-07", "04:59")


def test_fetches_at_most_weekly_and_on_request(conferences):
    start = datetime(2026, 11, 4, tzinfo=timezone.utc)
    cache, error = conferences.load_conference_data(now=start)
    assert error is None and len(conferences.calls) == 15
    assert cache["errors"]["icml"] == "404 Not Found"
    stored = json.loads(conferences._cache_path().read_text())
    assert set(stored["conferences"]["neurips"][0]) >= {"year", "deadlines", "start", "end"}

    conferences.load_conference_data(now=start + timedelta(days=6))
    assert len(conferences.calls) == 15, "a fresh cache is reused"
    conferences.load_conference_data(refresh=True, now=start + timedelta(days=1))
    assert len(conferences.calls) == 30, "Refresh always fetches"
    conferences.load_conference_data(now=start + timedelta(days=9))
    assert len(conferences.calls) == 45, "a week-old cache is refreshed"


def test_offline_keeps_the_last_data_and_reports_errors(conferences, monkeypatch):
    start = datetime(2026, 11, 4, tzinfo=timezone.utc)
    conferences.load_conference_data(now=start)

    def offline(url: str) -> str:
        raise OSError("network is unreachable")

    monkeypatch.setattr(conferences, "_fetch_text", offline)
    cache, error = conferences.load_conference_data(now=start + timedelta(days=30))
    assert cache and cache["conferences"]["neurips"], "stale data is still shown"
    assert "network is unreachable" in error

    conferences._cache_path().unlink()
    cache, error = conferences.load_conference_data(refresh=True, now=start)
    assert cache is None and "Could not fetch" in error
    data = conferences.conference_calendar({"year": 2026})
    assert data["events"] == [] and data["fetchedAt"] is None and data["error"]
