<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <out>
      <n1><xsl:value-of select="count(//book)"/></n1>
      <n2><xsl:value-of select="substring('hello', 1, 2)"/></n2>
      <n3><xsl:value-of select="1 + 2 * 3"/></n3>
      <n4><xsl:value-of select="string-length(title)"/></n4>
      <n5><xsl:value-of select="concat('a', 'b', 'c')"/></n5>
    </out>
  </xsl:template>
</xsl:stylesheet>
