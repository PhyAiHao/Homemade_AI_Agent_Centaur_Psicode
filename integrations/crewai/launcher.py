"""Launcher utility for the Centaur Psicode agent-brain process.

Ensures the Python agent-brain IPC server is running before the
CrewAI adapter tries to connect.
"""

from __future__ import annotations

import os
import socket
import subprocess
import time


def is_socket_alive(path: str) -> bool:
    """Check if a Unix socket is accepting connections."""
    try:
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(2)
        s.connect(path)
        s.close()
        return True
    except (FileNotFoundError, ConnectionRefusedError, OSError):
        return False


def ensure_agent_brain_running(
    socket_path: str = "/tmp/agent-ipc.sock",
    agent_brain_dir: str | None = None,
    timeout: int = 15,
) -> bool:
    """Start agent-brain if not already running.

    Args:
        socket_path: Path to the IPC Unix socket.
        agent_brain_dir: Directory containing the agent-brain package.
            If None, tries common locations.
        timeout: Seconds to wait for the server to start.

    Returns:
        True if agent-brain is available, False if it couldn't be started.
    """
    if is_socket_alive(socket_path):
        return True

    # Try to find agent-brain directory
    if agent_brain_dir is None:
        candidates = [
            os.path.expanduser("~/.agent/agent-brain"),
            os.path.join(os.path.dirname(__file__), "..", "..", "agent-brain"),
        ]
        for candidate in candidates:
            if os.path.isdir(candidate):
                agent_brain_dir = candidate
                break

    if agent_brain_dir is None:
        return False

    # Start the brain process
    env = {**os.environ, "AGENT_IPC_SOCKET": socket_path}
    try:
        subprocess.Popen(
            ["python", "-m", "agent_brain.ipc_server"],
            cwd=agent_brain_dir,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    except FileNotFoundError:
        return False

    # Wait for socket to appear
    for _ in range(timeout * 10):
        if is_socket_alive(socket_path):
            return True
        time.sleep(0.1)

    return False
