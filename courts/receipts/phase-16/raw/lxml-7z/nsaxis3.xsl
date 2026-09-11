<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:exsl="http://exslt.org/common">
  <xsl:template match="/">
    <out>
      <xsl:variable name="d"><xsl:element name="xsi:dummy" namespace="http://www.w3.org/2001/XMLSchema-instance"/></xsl:variable>
      <all><xsl:for-each select="exsl:node-set($d)/*/namespace::*">[<xsl:value-of select="local-name()"/>]</xsl:for-each></all>
      <cnt><xsl:value-of select="count(exsl:node-set($d)/*/namespace::*[local-name()='xsi'])"/></cnt>
    </out>
  </xsl:template>
</xsl:stylesheet>
