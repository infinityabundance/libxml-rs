<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:key name="bytitle" match="book" use="title"/>
  <xsl:template match="/">
    <out>
      <xsl:for-each select="key('bytitle', 'XSLT Cookbook')">
        <hit><xsl:value-of select="@id"/></hit>
      </xsl:for-each>
      <xsl:for-each select="key('bytitle', 'nope')">
        <miss><xsl:value-of select="@id"/></miss>
      </xsl:for-each>
    </out>
  </xsl:template>
</xsl:stylesheet>
