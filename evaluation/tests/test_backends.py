import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from taskbench.backends import CodeGraph, TextSearch, Unavailable, command


class BackendTests(unittest.TestCase):
    def test_command_timeout_and_output_limit(self):
        with self.assertRaises(TimeoutError):
            command([sys.executable, '-c', 'import time; time.sleep(3)'], cwd='.', timeout=.05)
        with self.assertRaises(RuntimeError):
            command([sys.executable, '-c', 'print("x"*10000)'], cwd='.', max_bytes=100)

    def test_driver_that_does_not_read_stdin_times_out(self):
        with self.assertRaises(TimeoutError):
            command([sys.executable, '-c', 'import time; time.sleep(3)'], cwd='.', stdin='x'*500000, timeout=.05)

    def test_missing_index_never_created(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaises(Unavailable):
                CodeGraph(Path(root))
            self.assertFalse((Path(root)/'.codegraph').exists())

    def test_codegraph_gaps_preserved(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root)/'.codegraph').mkdir()
            raw='**`x.ts`** — f\n```typescript\n1\tfunction f() {\n... (gap) ...\n9\t}\n```'
            with patch('taskbench.backends.command', return_value=(0,raw)):
                result=CodeGraph(Path(root)).search('f')
            self.assertIn('x.ts:1\tfunction f() {',result)
            self.assertIn('x.ts:9\t}',result)
            self.assertNotIn('x.ts:2',result)

    def test_rg_literal_terms_no_shell(self):
        with tempfile.TemporaryDirectory() as root:
            (Path(root)/'sample.rs').write_text('fn widget() {}\n')
            result=TextSearch(Path(root)).search('widget')
            self.assertIn('sample.rs:1\tfn widget() {}', result)

if __name__=='__main__':
    unittest.main()
