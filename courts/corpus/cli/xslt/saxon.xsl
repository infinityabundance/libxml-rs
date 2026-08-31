<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:saxon="http://icl.com/saxon">
<xsl:output method="text"/>
<xsl:template match="/">
<xsl:value-of select="saxon:systemId()"/>
<xsl:value-of select="saxon:line-number()"/>
<xsl:value-of select="saxon:line-number(//book)"/>
<xsl:value-of select="saxon:evaluate('2 + 3')"/>
<xsl:value-of select="saxon:eval(saxon:expression('5 * 5'))"/>
</xsl:template>
</xsl:stylesheet>
