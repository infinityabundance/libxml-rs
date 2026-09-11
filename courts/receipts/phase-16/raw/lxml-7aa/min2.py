import unittest, io
from lxml.tests import test_elementpath
from lxml import etree
loader = unittest.TestLoader()
suite = unittest.TestSuite()
suite.addTests(loader.loadTestsFromName('lxml.tests.test_elementpath.EtreeElementPathTestCase.test_find'))
unittest.TextTestRunner(verbosity=0, stream=io.StringIO()).run(suite)
html = etree.HTML("<html><head><title>test</title></head><body><h1>page title</h1></body></html>")
print(etree.tostring(html, method="html"))
