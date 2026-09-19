import unittest
from experiment import BM25, rank, tokens, stem


class ExperimentTests(unittest.TestCase):
    def test_documentation_index_keeps_its_own_corpus_and_candidate_offset(self):
        docs=[[('callback operator credentials',1)],[('migration environment recovery',1)]]
        ordinary=BM25(docs).scores(['callback'])
        sparse=BM25(docs,offset=10000).scores(['callback'])
        self.assertEqual(sparse,{i+10000:value for i,value in ordinary.items()})
        self.assertGreater(sparse[10000],0)

    def test_exact_lane_does_not_depend_on_content_or_diversity(self):
        candidates=[dict(id=str(i),path=path,name='target',qualified_name='target',start=1,end=2,kind='function') for i,path in enumerate(['a.rs','b.rs'])]
        indexes={'metadata':BM25([[('target',1)],[('target target target',1)]])}
        hits=rank(candidates,indexes,'target',dict(weights={'metadata':1},diversity=.25))
        self.assertEqual([h['path'] for h in hits],['a.rs','b.rs'])

    def test_identifier_boundaries_and_frozen_inflections(self):
        self.assertEqual(tokens('HTTPServer::loadPdsSecret snake_case'),['http','server','load','pds','secret','snake','case'])
        self.assertEqual(stem('retries'),stem('retry'))
        self.assertEqual(stem('removed'),stem('remove'))
        self.assertEqual(stem('processing'),stem('process'))

if __name__=='__main__':unittest.main()
