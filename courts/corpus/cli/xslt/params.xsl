<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:param name="who" select="'nobody'"/>
  <xsl:param name="times" select="1"/>
  <xsl:template match="/">
    <out>
      <xsl:for-each select="//book[position() &lt;= $times]">
        <greet><xsl:value-of select="concat('hello ', $who)"/></greet>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>
