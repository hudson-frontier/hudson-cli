"""Exercise a native terminal session against a local synthetic agent API."""
import argparse
import http.server
import json
import os
from pathlib import Path
import pty
import select
import subprocess
import threading
import time

RUN = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
OPERATION = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb'
TOKEN = 'hudson-terminal-fixture-token'


def exercise(binary: Path, namespace: bool):
    requests = []
    status = 'waiting'

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def send(self, body, code=200):
            data = json.dumps(body).encode()
            self.send_response(code)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def do_POST(self):
            nonlocal status
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append((self.path, body, self.headers.get('Authorization')))
            if self.path == '/runs' and body['input'] == 'Rejected task':
                self.send({'error': TOKEN}, 403)
                return
            if self.path.endswith('/input'):
                assert body['question_id'] == 7 and body['input'] == 'Boston'
                status = 'completed'
            self.send({'run_id': RUN})

        def do_GET(self):
            requests.append((self.path, None, self.headers.get('Authorization')))
            if '/events?' in self.path:
                if 'after=0&' in self.path:
                    self.send([{'sequence': 1, 'run_id': RUN, 'event_type': 'run_waiting\nFORGED_EVENT_PROMPT\t\u061c\u200e\u200f\u202e\u2066\u2028\u2029',
                                'payload': {'message': '\x1b]52;c;secret\x07'}}])
                else:
                    self.send([])
            else:
                self.send({'id': RUN, 'status': status,
                           'reason': 'reason\nFORGED_REASON_PROMPT\t\u061c\u200e\u200f\u202e\u2066\u2028\u2029',
                           'wait': {'type': 'user_input', 'question_id': 7, 'prompt': 'Which city?'}
                           if status == 'waiting' else None,
                           'result': {'city': 'Boston'} if status == 'completed' else None})

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    master, slave = pty.openpty()
    environment = dict(os.environ, HUDSON_TERMINAL_TOKEN=TOKEN)
    for name in ['HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'all_proxy']:
        environment.pop(name, None)
    arguments = [str(binary)] + (['agent'] if namespace else [])
    arguments += ['--url', f'http://127.0.0.1:{server.server_port}',
                  '--api-token-env', 'HUDSON_TERMINAL_TOKEN']
    if namespace:
        arguments.append('session')
    child = subprocess.Popen(arguments, stdin=slave, stdout=slave, stderr=slave, env=environment)
    os.close(slave)
    output = bytearray()

    def wait_for(text, since=0):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if text.encode() in output[since:]:
                return
            ready, _, _ = select.select([master], [], [], .1)
            if ready:
                output.extend(os.read(master, 65536))
                if len(output) > 2 * 1024 * 1024:
                    raise AssertionError('Terminal produced excessive output')
        raise AssertionError('Missing terminal response: ' + text)

    def send(text):
        before = len(output)
        os.write(master, (text + '\n').encode())
        wait_for('hudson> ', before)

    try:
        wait_for('hudson> ')
        send('Please find a city')
        assert b'Which city?' in output and b'\\u001b' in output
        assert b'\x1b]52' not in output
        assert b'\r\nFORGED_EVENT_PROMPT' not in output
        assert b'\r\nFORGED_REASON_PROMPT' not in output
        assert b'run_waiting\\u{a}FORGED_EVENT_PROMPT\\u{9}' in output
        assert b'Reason: reason\\u{a}FORGED_REASON_PROMPT\\u{9}' in output
        for control in '\u061c\u200e\u200f\u202e\u2066\u2028\u2029':
            assert control.encode() not in output
        send('/reply 7 Boston')
        assert b'Result:' in output and b'Boston' in output
        send('Rejected task')
        assert b'403 Forbidden' in output
        send('/status')
        send('/open ' + RUN)
        send('/deny ' + OPERATION)
        send('/resume')
        send('/cancel')
        send('/unknown')
        assert b'unknown command' in output
        os.write(master, b'/quit\n')
        assert child.wait(timeout=5) == 0
        submissions = [body for path, body, _ in requests if path == '/runs']
        assert len(submissions) == 2
        assert set(submissions[0]) == {'input', 'request_key'}
        assert all(auth == 'Bearer ' + TOKEN for _, _, auth in requests)
        assert len([1 for path, _, _ in requests if path.endswith('/input')]) == 1
        assert any(path.endswith('/approval') and body == {'approved': False}
                   for path, body, _ in requests)
        assert TOKEN not in output.decode()
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        os.close(master)
        server.shutdown()
        server.server_close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    for namespace in [False, True]:
        exercise(args.binary.resolve(), namespace)
    result = subprocess.run([str(args.binary.resolve())], input='/quit\n',
                            capture_output=True, text=True, timeout=10)
    assert result.returncode == 1 and 'requires a terminal' in result.stderr
    print('Terminal sessions passed: task, reply, controls, auth, escaping and compatibility')


if __name__ == '__main__':
    main()
