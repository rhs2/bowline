package com.bowline.billing.document;

import com.lowagie.text.Document;
import com.lowagie.text.Element;
import com.lowagie.text.Phrase;
import com.lowagie.text.pdf.BaseFont;
import com.lowagie.text.pdf.ColumnText;
import com.lowagie.text.pdf.PdfContentByte;
import com.lowagie.text.pdf.PdfPageEventHelper;
import com.lowagie.text.pdf.PdfTemplate;
import com.lowagie.text.pdf.PdfWriter;
import java.io.IOException;

/**
 * Draws a footer on every page: a caption on the left and "Page X of Y" on the right.
 * The total is written into a template once the document closes, the standard
 * OpenPDF technique for page counts.
 */
public class PageFooter extends PdfPageEventHelper {

    private static final float FONT_SIZE = 8f;
    private static final float TEMPLATE_WIDTH = 30f;
    private static final float TEMPLATE_HEIGHT = 12f;

    private final String caption;
    private PdfTemplate total;
    private BaseFont baseFont;

    public PageFooter(String caption) {
        this.caption = caption;
    }

    @Override
    public void onOpenDocument(PdfWriter writer, Document document) {
        total = writer.getDirectContent().createTemplate(TEMPLATE_WIDTH, TEMPLATE_HEIGHT);
        try {
            baseFont = BaseFont.createFont(BaseFont.HELVETICA, BaseFont.WINANSI, BaseFont.NOT_EMBEDDED);
        } catch (IOException e) {
            throw new IllegalStateException("built-in Helvetica is unavailable", e);
        }
    }

    @Override
    public void onEndPage(PdfWriter writer, Document document) {
        PdfContentByte canvas = writer.getDirectContent();
        float y = document.bottom() - 22;

        canvas.setLineWidth(0.4f);
        canvas.setColorStroke(PdfStyles.RULE);
        canvas.moveTo(document.left(), y + 12);
        canvas.lineTo(document.right(), y + 12);
        canvas.stroke();

        ColumnText.showTextAligned(canvas, Element.ALIGN_LEFT, new Phrase(caption, PdfStyles.SMALL), document.left(), y, 0);

        String text = "Page " + writer.getPageNumber() + " of";
        float x = document.right() - TEMPLATE_WIDTH + 6;
        ColumnText.showTextAligned(canvas, Element.ALIGN_RIGHT, new Phrase(text, PdfStyles.SMALL), x, y, 0);
        canvas.addTemplate(total, x + 2.5f, y);
    }

    @Override
    public void onCloseDocument(PdfWriter writer, Document document) {
        total.beginText();
        total.setFontAndSize(baseFont, FONT_SIZE);
        total.setColorFill(PdfStyles.MUTED);
        total.setTextMatrix(0, 0);
        total.showText(String.valueOf(writer.getPageNumber() - 1));
        total.endText();
    }
}
