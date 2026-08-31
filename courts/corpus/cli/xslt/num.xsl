<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="text"/>
<xsl:template match="/">
  <xsl:value-of select="1234567.891"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.1 + 0.2"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.0 div 3.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1e20"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1e-5"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="123456789012345678901234567890"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1e100"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-1e100"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.5e-100"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1e9"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="999999999.9999999"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.00001"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="9.99e-6"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2147483646"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2147483648"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-2147483647"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-2147483649"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="123456789.123456789"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.000123456789"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.5"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.0 div 7.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2.675"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="3.141592653589793"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-0.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.30000000000000004"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2.2250738585072014e-308"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="5e-324"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="100000000000000000000"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1 div 3"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2 div 3"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.7"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2.0 div 7.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-1.5"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.0 div 0.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-1.0 div 0.0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0 div 0"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.7976931348623157e308"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-1.7976931348623157e308"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="4.9e-324"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.0e-5"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.0000099999"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1.0000000000001e9"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="2147483647"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-2147483648"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="999999999999999"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.25"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="0.125"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="3.14"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="-3.14"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="1234567.891000000061467"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('+5')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('5e-324')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('0.1234567890123456789012345')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number(' 12.5 ')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('12x')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('.5')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="number('5.')"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="string(5.)"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="string(.5)"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="string(5e-324)"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="string(-0.0)"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="string(1 div 0)"/>
  <xsl:text>&#10;</xsl:text>
  <xsl:value-of select="string(0 div 0)"/>
  <xsl:text>&#10;</xsl:text>
</xsl:template>
</xsl:stylesheet>
