import sys, unittest, importlib
mod = sys.argv[1]
m = importlib.import_module('lxml.tests.' + mod)
from lxml.tests import test_htmlparser
loader = unittest.TestLoader()
suite = unittest.TestSuite()
suite.addTests(loader.loadTestsFromModule(m))
suite.addTests(loader.loadTestsFromModule(test_htmlparser))
res = unittest.TextTestRunner(verbosity=0).run(suite)
bad = [t.id() for t, tb in res.failures + res.errors if 'test_module_HTML' in t.id() or 'module_parse_html' in t.id()]
print("MODULE", mod, "moduleHTML_failures", len(bad))
for b in bad: print("   ", b)
