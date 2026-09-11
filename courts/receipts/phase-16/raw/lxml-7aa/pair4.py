import sys, unittest, io
from lxml.tests import test_elementpath
loader = unittest.TestLoader()
name = sys.argv[1]
suite = unittest.TestSuite()
suite.addTests(loader.loadTestsFromName(name))
suite.addTests(loader.loadTestsFromName('lxml.tests.test_htmlparser.HtmlParserTestCase.test_module_HTML'))
res = unittest.TextTestRunner(verbosity=0, stream=io.StringIO()).run(suite)
print("NFAIL", len(res.failures)+len(res.errors))
