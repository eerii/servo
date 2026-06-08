# Copyright 2013 The Servo Project Developers. See the COPYRIGHT
# file at the top-level directory of this distribution.
#
# Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
# http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
# <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
# option. This file may not be copied, modified, or distributed
# except according to those terms.

import http.server
import os
import socket
import socketserver
import subprocess
import sys
import time
from subprocess import TimeoutExpired
from threading import Thread

import pytest

from . import utils

WAIT_BETWEEN_ATTEMPTS = 1 / 8  # seconds
CONNECTION_TIMEOUT = 5  # seconds


class _ExpectedFailuresPlugin:
    def __init__(self, patterns):
        self._patterns = patterns

    @pytest.hookimpl(trylast=True)
    def pytest_collection_modifyitems(self, items):
        for item in items:
            for pattern in self._patterns:
                if pattern in item.nodeid:
                    item.add_marker(
                        pytest.mark.xfail(reason=f"Expected failure: {pattern}")
                    )
                    break


def pytest_configure(config):
    failures_file = os.path.join(os.path.dirname(__file__), "expected_failures.txt")
    if not os.path.exists(failures_file):
        return
    patterns = []
    with open(failures_file) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                patterns.append(line)
    if patterns:
        config.pluginmanager.register(
            _ExpectedFailuresPlugin(patterns), name="expected-failures"
        )

web_servers = []
web_server_threads = []


def pytest_addoption(parser):
    parser.addoption("--servo-binary", help="Path to the servoshell binary")
    parser.addoption("--script-path", help="Path to the servo python library")




def pytest_sessionstart(session: pytest.Session):
    if hasattr(session.config, "workerinput"):
        return
    # The web servers for the test files need to be started before we spawn the workers running the tests
    _start_web_servers(session.config)


def pytest_sessionfinish(session: pytest.Session):
    if hasattr(session.config, "workerinput"):
        return
    _stop_web_servers()


@pytest.fixture(scope="session")
def servo_binary(request):
    binary = request.config.getoption("--servo-binary")
    if not binary:
        pytest.fail("The --servo-binary option must be specified")
    return binary


@pytest.fixture(scope="session")
def test_dir(request):
    return _test_dir(request.config)


def _test_dir(config):
    path = config.getoption("--script-path")
    if not path:
        pytest.fail("The --script-path option must be specified")
    return os.path.join(path, "devtools_tests")


@pytest.fixture(scope="session", autouse=True)
def devtools_port(worker_id):
    base_port = 6000
    if worker_id == "master":
        return base_port

    worker_num = int(worker_id.replace("gw", ""))
    port = base_port + worker_num

    # Set the thread local port in utils, which is used by the Devtools class
    # This avoid having to pass it as a parameter in every invocation of connect
    utils.DEVTOOLS_PORT = port

    return port


@pytest.fixture(scope="session", autouse=True)
def _start_servoshell_for_geckordp(request, servo_binary, worker_id):
    try:
        browser = request.config.getoption("--browser")
        base_port = request.config.getoption("--remote-port")
    except (ValueError, AttributeError):
        yield
        return

    if browser != "external":
        yield
        return

    is_xdist_controller = (
        request.config.pluginmanager.hasplugin("xdist")
        and not hasattr(request.config, "workerinput")
    )
    if is_xdist_controller:
        yield
        return

    if worker_id == "master":
        port = base_port
    else:
        worker_num = int(worker_id.replace("gw", ""))
        port = base_port + worker_num

    import geckordp.tests.helpers.constants as geckordp_constants

    geckordp_constants.REMOTE_PORT = port

    env = os.environ.copy()
    env["RUST_LOG"] = "error,devtools=warn"
    process = subprocess.Popen(
        [servo_binary, "--headless", f"--devtools={port}", "servo:newtab"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    for _ in range(40):
        try:
            with socket.create_connection(("127.0.0.1", port)):
                time.sleep(0.25)
                break
        except Exception:
            time.sleep(1 / 8)
    else:
        process.terminate()
        try:
            process.wait(5)
        except Exception:
            process.kill()
        raise TimeoutError(f"Couldn't connect to servoshell on port {port}")

    yield

    process.terminate()
    try:
        process.wait(timeout=CONNECTION_TIMEOUT)
    except TimeoutExpired:
        print(
            "Warning: servoshell for geckordp did not terminate", file=sys.stderr
        )
        process.kill()


@pytest.fixture(scope="session")
def web_server_urls(worker_id):
    base_urls = [f"http://{utils.SERVER_ADDRESS}:{port}" for port in utils.WEB_SERVERS]

    # For workers other than the one starting the web servers, we want to poll until they are available
    if worker_id != "master":
        for port in utils.WEB_SERVERS:
            for _ in range(int(CONNECTION_TIMEOUT / WAIT_BETWEEN_ATTEMPTS)):
                try:
                    with socket.create_connection((utils.SERVER_ADDRESS, port)):
                        break
                except Exception:
                    time.sleep(WAIT_BETWEEN_ATTEMPTS)
            else:
                raise TimeoutError(f"Couldn't connect to web server at {utils.SERVER_ADDRESS}:{port}")

    return base_urls


def _start_web_servers(config):
    if web_servers:
        return
    directory = _test_dir(config)

    class Handler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=directory, **kwargs)

        def log_message(self, format, *args):
            if utils.LOG_REQUESTS:
                return super().log_message(format, *args)

    def server_thread(i):
        # There may be client sockets still open in TIME_WAIT state from previous tests, and they may stay open for
        # some minutes. Set SO_REUSEADDR to avoid bind failure with EADDRINUSE in these cases.
        # <https://stackoverflow.com/questions/14388706>
        socketserver.TCPServer.allow_reuse_address = True

        # Listen on all IPv4 interfaces.
        port = utils.WEB_SERVERS[i]
        web_server = socketserver.TCPServer((utils.SERVER_ADDRESS, port), Handler)

        web_servers.append(web_server)
        web_server.serve_forever()

    # Start a web server for the test.
    for i in range(len(utils.WEB_SERVERS)):
        thread = Thread(target=server_thread, args=[i])
        web_server_threads.append(thread)
        thread.start()

    for port in utils.WEB_SERVERS:
        for _ in range(int(CONNECTION_TIMEOUT / WAIT_BETWEEN_ATTEMPTS)):
            try:
                with socket.create_connection((utils.SERVER_ADDRESS, port)):
                    break
            except Exception:
                time.sleep(WAIT_BETWEEN_ATTEMPTS)


def _stop_web_servers():
    for server in web_servers:
        if server:
            server.shutdown()
            server.server_close()
    for thread in web_server_threads:
        if thread:
            thread.join()


@pytest.fixture
def run_servoshell(servo_binary, devtools_port):
    process = None

    def run(*, url):
        nonlocal process

        # Change this setting if you want to debug Servo.
        os.environ["RUST_LOG"] = "error,devtools=warn"

        # Run servoshell.
        process = subprocess.Popen([servo_binary, "--headless", f"--devtools={devtools_port}", url])

        # Try to connect to the devtools server.
        for _ in range(int(CONNECTION_TIMEOUT / WAIT_BETWEEN_ATTEMPTS)):
            print(".", end="", file=sys.stderr)
            try:
                with socket.create_connection((utils.SERVER_ADDRESS, devtools_port)) as stream:
                    stream.recv(4096)  # FIXME: geckordp workaround
                    stream.shutdown(socket.SHUT_RDWR)
                print("+", end="", file=sys.stderr, flush=True)
                return process
            except Exception:
                time.sleep(WAIT_BETWEEN_ATTEMPTS)
        raise TimeoutError(
            f"Couldn't connect to the devtools server at {utils.SERVER_ADDRESS}:{devtools_port} in {CONNECTION_TIMEOUT}s"
        )

    yield run

    # Terminate servoshell.
    if process:
        process.terminate()
        try:
            process.wait(timeout=CONNECTION_TIMEOUT)
        except TimeoutExpired:
            print("Warning: servoshell did not terminate", file=sys.stderr)
            process.kill()
