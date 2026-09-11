<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:exsl="http://exslt.org/common">
  <xsl:template match="/">
    <out>
      <xsl:variable name="d"><xsl:element name="xsi:dummy" namespace="http://www.w3.org/2001/XMLSchema-instance"/></xsl:variable>
      <n><xsl:copy-of select="exsl:node-set($d)/*/namespace::*[local-name()='xsi']"/></n>
    </out>
  </xsl:template>
</xsl:stylesheet>
