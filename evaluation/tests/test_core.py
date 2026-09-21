import copy
import json
import tempfile
import unittest
from pathlib import Path
from taskbench.core import bounded, coverage, delivered_lines, digest, grade, grading_packet, read_source, source_path, validate


class CoreTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'a.rs').write_text('first\nsecond\nthird\n')
        self.task = dict(id='x', repo='test', family='f', split='dev', kind='debug', prompt='Why?')
        self.oracle = dict(regions=[dict(id='r', path='a.rs', start=1, end=2,
            file_sha256=digest(b'first\nsecond\nthird\n'), sha256=digest(b'first\nsecond\n'))],
            criteria=[dict(id='explain', description='Explain the cause', regions=['r'])],
            relationships=[dict(description='source to target', regions=['r'])])

    def test_source_drift(self):
        validate([self.task], {'x': self.oracle}, {'test': self.root})
        (self.root / 'a.rs').write_text('changed\n')
        with self.assertRaises(ValueError):
            validate([self.task], {'x': self.oracle}, {'test': self.root})

    def test_public_gold_and_split_separation(self):
        leaked = {**self.task, 'required_path': 'a.rs'}
        with self.assertRaises(ValueError):
            validate([leaked], {'x': self.oracle}, {'test': self.root})
        other = {**self.task, 'id': 'y', 'split': 'heldout'}
        with self.assertRaises(ValueError):
            validate([self.task, other], {'x': self.oracle, 'y': self.oracle}, {'test': self.root})

    def test_path_escape(self):
        for path in ('../outside', '/etc/passwd'):
            with self.assertRaises(ValueError):
                source_path(self.root, path)
        (self.root / 'escape').symlink_to('/etc/passwd')
        with self.assertRaises(ValueError):
            source_path(self.root, 'escape')

    def test_delivered_not_declared(self):
        seen = delivered_lines('a.rs:1\tfirst\na.rs:2\tseco\na.rs:3\tthird', self.root)
        self.assertEqual(seen, {('a.rs', 1), ('a.rs', 3)})
        result = coverage(self.oracle, seen)
        self.assertEqual(result['region_coverage']['r'], .5)
        self.assertFalse(result['evidence_ready'])
        self.assertIsNone(result['task_success'])

    def test_native_json_counts_only_verified_delivered_lines(self):
        from taskbench.runner import candidates
        value={'items':[{'node':{'path':'a.rs','start_line':1,'signature':'fake:999 target'},
            'snippet':{'start_line':1,'source_hash':digest(b'first\nsecond\nthird\n'),'lines':['first']},
            'excerpts':[{'role':'body','snippet':{'start_line':3,'lines':['third','not in file']}}]},
            {'node':{'path':'a.rs','start_line':2},'snippet':{'start_line':2,'lines':['seco']}},
            {'node':{'path':'../escape','start_line':1},'snippet':{'start_line':1,'lines':['first']}},
            {'node':{'path':'a.rs','start_line':2},'snippet':{'start_line':2,'source_hash':'stale','lines':['second']}},
            {'node':{'path':'a.rs','start_line':True},'snippet':{'start_line':True,'lines':['first']}},
            None]}
        encoded=json.dumps(value,ensure_ascii=False,separators=(',',':'))
        self.assertEqual(delivered_lines(encoded,self.root),{('a.rs',1),('a.rs',3)})
        self.assertEqual(candidates(encoded),[('a.rs',1),('../escape',1)])
        self.assertEqual(delivered_lines(encoded[:-1],self.root),set())
        self.assertEqual(candidates(encoded[:-1]),[])
        (self.root/'a.rs').write_text('first\nsecond\nchanged\n')
        self.assertEqual(delivered_lines(encoded,self.root),set())

    def test_budget_unicode_and_read(self):
        self.assertEqual(bounded('éé', 3), ('é', True))
        self.assertEqual(read_source(self.root, 'a.rs', 2, 1), 'a.rs:2\tsecond')
        with self.assertRaises(ValueError):
            read_source(self.root, 'a.rs', 0)

    def test_blind_grade(self):
        trial = dict(arm='secret-engine', answer='Cause', citations=[dict(path='a.rs', line=1)], seen=[['a.rs', 1]])
        packet = grading_packet(self.task, self.oracle, trial)
        self.assertNotIn('arm', packet)
        judgments = dict(packet_sha256=packet['packet_sha256'], reviewer='human', criteria={'explain': True}, unsupported_claims=False)
        self.assertTrue(grade(packet, judgments)['task_success'])
        judgments['criteria'] = {}
        with self.assertRaises(ValueError):
            grade(packet, judgments)
        altered = copy.deepcopy(packet)
        altered['answer'] = 'Different'
        with self.assertRaises(ValueError):
            grade(altered, judgments)
        with self.assertRaises(ValueError):
            grading_packet(self.task, self.oracle, {'answer': None})

    def test_unseen_citations_fail(self):
        packet = grading_packet(self.task, self.oracle, dict(answer='Cause', citations=[dict(path='a.rs', line=3)], seen=[['a.rs', 1]]))
        judgments = dict(packet_sha256=packet['packet_sha256'], reviewer='human', criteria={'explain': True}, unsupported_claims=False)
        self.assertFalse(grade(packet, judgments)['task_success'])


if __name__ == '__main__':
    unittest.main()
