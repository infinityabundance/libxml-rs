from lxml import etree
s = etree.Schematron(etree.XML('''<schema xmlns="http://www.ascc.net/xml/schematron">
  <pattern name="p"><rule context="*">
     <report test="@*[not(name()='id')]">Attribute is forbidden</report>
  </rule></pattern></schema>'''))
xml = etree.XML('<AAA name="aaa"><BBB id="bbb"/><CCC color="ccc"/></AAA>')
print("validate ->", s.validate(xml))
