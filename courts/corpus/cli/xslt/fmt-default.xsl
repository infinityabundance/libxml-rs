<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="text"/>
<xsl:template match="/">
<xsl:value-of select="format-number(1234567.891, '#,##0.00')"/>
<xsl:value-of select="format-number(-1234.5, '#,##0.00;(#,##0.00)')"/>
<xsl:value-of select="format-number(0.055, '0.00%')"/>
<xsl:value-of select="format-number(0.055, '0.00&#x2030;')"/>
<xsl:value-of select="format-number(42, '0000')"/>
<xsl:value-of select="format-number(3.14159, '#.##')"/>
<xsl:value-of select="format-number(0 div 0, '0.00')"/>
<xsl:value-of select="format-number(1 div 0, '0.00')"/>
<xsl:value-of select="format-number(-1 div 0, '0.00')"/>
<xsl:value-of select="format-number(1234.5, &quot;'$'#,##0.00&quot;)"/>
<xsl:value-of select="format-number(1234.5, '#,##0.00 USD')"/>
<xsl:value-of select="format-number(42, '.#')"/>
<xsl:value-of select="format-number(1234.5, '0.##0')"/>
<xsl:value-of select="format-number(1234.5, '0;0;0')"/>
<xsl:value-of select="format-number(0.000000001, '0.000000000')"/>
<xsl:value-of select="format-number(123.456, '0.00#')"/>
<xsl:value-of select="format-number(2, '00.00')"/>
<xsl:value-of select="format-number(1234.5, '0,000')"/>
</xsl:template>
</xsl:stylesheet>
