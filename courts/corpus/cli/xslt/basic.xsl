<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/">
    <catalog>
      <xsl:for-each select="library/book">
        <book id="{@id}">
          <xsl:value-of select="title"/>
          <xsl:if test="author">
            <author><xsl:value-of select="author"/></author>
          </xsl:if>
        </book>
      </xsl:for-each>
      <count><xsl:value-of select="count(library/book)"/></count>
    </catalog>
  </xsl:template>
</xsl:stylesheet>
