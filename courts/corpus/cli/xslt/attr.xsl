<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <out>
      <xsl:for-each select="//book">
        <b id="{@id}" sv="{string(@id)}">
          <xsl:if test="@id = 'b2'"><match>yes</match></xsl:if>
        </b>
      </xsl:for-each>
      <s1><xsl:value-of select="string(//book[@id='b1']/title)"/></s1>
    </out>
  </xsl:template>
</xsl:stylesheet>
