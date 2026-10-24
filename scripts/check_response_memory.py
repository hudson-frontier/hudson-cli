"""Check Linux response formatting with a bounded heap and large formatted output."""
import argparse
import http.server
import os
from pathlib import Path
import resource
import subprocess
import sys
import tempfile
import threading

RUN = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
HEAP_LIMIT = 128 * 1024 * 1024


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', required=True, type=Path)
    parser.add_argument('--child-url')
    parser.add_argument('--legacy', action='store_true')
    args = parser.parse_args()
    if args.child_url:
        resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
        resource.setrlimit(resource.RLIMIT_DATA, (HEAP_LIMIT, HEAP_LIMIT))
        binary = args.binary_dir.resolve() / ('hudson-cli' if args.legacy else 'hudson')
        command = [str(binary)] + ([] if args.legacy else ['agent'])
        command += ['--url', args.child_url, 'get', RUN]
        os.execve(str(binary), command, {'PATH': os.environ['PATH'], 'MALLOC_ARENA_MAX': '1'})

    # This valid result is well below the 8 MiB response limit, but its indentation
    # expands beyond the heap budget. Do not construct the formatted result here.
    body = ('{"id":"' + RUN + '","status":"completed","result":' +
            '[' * 100 + '0,' * 699999 + '0' + ']' * 100 + '}').encode()
    requests = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            requests.append(self.path)
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        for legacy in [False, True]:
            command = [sys.executable, str(Path(__file__).resolve()), '--binary-dir',
                       str(args.binary_dir.resolve()), '--child-url',
                       f'http://127.0.0.1:{server.server_port}']
            if legacy:
                command.append('--legacy')
            # Check output was written without buffering it in the verifier's heap.
            with tempfile.TemporaryFile(dir=os.environ.get('HUDSON_MEMORY_TEST_TMPDIR')) as output:
                result = subprocess.run(command, stdout=output, stderr=subprocess.PIPE, timeout=30)
                assert result.returncode == 0, (legacy, result.returncode, result.stderr[:1000])
                assert not result.stderr, result.stderr[:1000]
                assert 128 * 1024 * 1024 < output.tell() < 256 * 1024 * 1024
        assert requests == [f'/runs/{RUN}', f'/runs/{RUN}'], requests
    finally:
        server.shutdown()
        server.server_close()
    print('Response formatting passed under a 128 MiB heap limit for both agent entrypoints')


if __name__ == '__main__':
    main()
