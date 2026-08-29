<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template name="greet">
    <xsl:param name="who"/>
    <xsl:param name="punct">!</xsl:param>
    <msg><xsl:value-of select="concat('hello ', $who, $punct)"/></msg>
  </xsl:template>
  <xsl:template match="/">
    <out>
      <xsl:call-template name="greet">
        <xsl:with-param name="who" select="'world'"/>
      </xsl:call-template>
      <xsl:call-template name="greet">
        <xsl:with-param name="who" select="//book[1]/title"/>
        <xsl:with-param name="punct" select="'?'"/>
      </xsl:call-template>
    </out>
  </xsl:template>
</xsl:stylesheet>
