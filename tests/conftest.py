import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from calcbridge.device import Calculator  # noqa: E402
from calcbridge.protocol.nnse import Link  # noqa: E402
from calcbridge.sim import SimCalculator, SimTransport  # noqa: E402


def pytest_addoption(parser):
    parser.addoption("--hardware", action="store_true", help="also run tests against a real CX II")


def pytest_collection_modifyitems(config, items):
    if config.getoption("--hardware"):
        return
    skip = pytest.mark.skip(reason="needs --hardware and a connected calculator")
    for item in items:
        if "hardware" in item.keywords:
            item.add_marker(skip)


@pytest.fixture(autouse=True)
def fast_link(monkeypatch, request):
    if "hardware" in request.keywords:
        return
    # Keep retry timeouts short in tests; the simulator answers instantly.
    monkeypatch.setattr(Link, "ACK_TIMEOUT_MS", 200)
    monkeypatch.setattr(Link, "PROBE_TIMEOUT_MS", 40)


@pytest.fixture
def sim():
    return SimCalculator(idle_drop_s=0)


@pytest.fixture
def calc(sim, tmp_path):
    c = Calculator(transport=SimTransport(sim), state_file=str(tmp_path / "state.json"))
    yield c
    c.close()
