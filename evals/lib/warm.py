"""Builds frappe-lsp's index cache for the bench in the current directory.

A run's agent asks the server within seconds; a cold index of frappe and press
takes longer. A real bench keeps a warm cache, so the eval starts from one too.
"""

import glob
import json
import os
import subprocess
import sys
import threading
import time

TIMEOUT_SECONDS = 90


def send(server, message):
	body = json.dumps({"jsonrpc": "2.0", **message}).encode()
	server.stdin.write(b"Content-Length: %d\r\n\r\n%s" % (len(body), body))
	server.stdin.flush()


def read(server):
	headers = {}
	while line := server.stdout.readline().strip():
		name, _, value = line.decode().partition(":")
		headers[name.lower()] = value.strip()
	if "content-length" not in headers:
		return None
	return json.loads(server.stdout.read(int(headers["content-length"])))


def answer_requests(server, initialized):
	"""Reply null to server requests, so none of them blocks the index."""
	while (message := read(server)) is not None:
		if "id" in message and "method" in message:
			send(server, {"id": message["id"], "result": None})
		elif message.get("id") == 1:
			initialized.set()


def cpu_ticks(pid):
	with open(f"/proc/{pid}/stat") as stat:
		fields = stat.read().rsplit(")", 1)[1].split()
	return int(fields[11]) + int(fields[12])


def wait_until_idle(server):
	deadline = time.monotonic() + TIMEOUT_SECONDS
	previous = None
	while time.monotonic() < deadline:
		time.sleep(2)
		state = (cpu_ticks(server.pid), cache_size())
		if state == previous and state[1]:
			return
		previous = state
	sys.exit("frappe-lsp did not finish indexing in time")


def cache_size():
	return sum(os.path.getsize(path) for path in glob.glob(".frappe-lsp/index.v*.bin"))


def main(launcher):
	server = subprocess.Popen([launcher], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
	initialized = threading.Event()
	threading.Thread(target=answer_requests, args=(server, initialized), daemon=True).start()
	root = "file://" + os.getcwd()
	send(server, {"id": 1, "method": "initialize", "params": {
		"processId": os.getpid(),
		"rootUri": root,
		"workspaceFolders": [{"uri": root, "name": "bench"}],
		"capabilities": {},
	}})
	# The server drops `initialized` if it comes before this response.
	initialized.wait(timeout=10)
	send(server, {"method": "initialized", "params": {}})
	wait_until_idle(server)
	send(server, {"id": 2, "method": "shutdown"})
	send(server, {"method": "exit"})
	server.wait(timeout=10)
	print(f"frappe-lsp index cache: {cache_size()} bytes", file=sys.stderr)


main(sys.argv[1])
