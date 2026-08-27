package com.bowline.billing.document;

import com.lowagie.text.Element;
import com.lowagie.text.Font;
import com.lowagie.text.Phrase;
import com.lowagie.text.Rectangle;
import com.lowagie.text.pdf.PdfPCell;
import java.awt.Color;

/** Fonts, colours and cell factories shared by every PDF the service renders. */
public final class PdfStyles {

    public static final Color INK = new Color(0x1F, 0x24, 0x2B);
    public static final Color MUTED = new Color(0x5C, 0x66, 0x70);
    public static final Color RULE = new Color(0xB9, 0xC2, 0xCB);
    public static final Color HEADER_FILL = new Color(0xE8, 0xED, 0xF2);
    public static final Color STRIPE_FILL = new Color(0xF6, 0xF8, 0xFA);

    public static final Font TITLE = new Font(Font.HELVETICA, 20, Font.BOLD, INK);
    public static final Font HEADING = new Font(Font.HELVETICA, 15, Font.BOLD, INK);
    public static final Font BODY = new Font(Font.HELVETICA, 9.5f, Font.NORMAL, INK);
    public static final Font BODY_BOLD = new Font(Font.HELVETICA, 9.5f, Font.BOLD, INK);
    public static final Font LABEL = new Font(Font.HELVETICA, 8, Font.BOLD, MUTED);
    public static final Font SMALL = new Font(Font.HELVETICA, 8, Font.NORMAL, MUTED);
    public static final Font TABLE_HEADER = new Font(Font.HELVETICA, 8.5f, Font.BOLD, INK);
    public static final Font TOTAL = new Font(Font.HELVETICA, 11, Font.BOLD, INK);

    private PdfStyles() {}

    /** A borderless cell, used for layout tables. */
    public static PdfPCell plain(Phrase phrase) {
        PdfPCell cell = new PdfPCell(phrase);
        cell.setBorder(Rectangle.NO_BORDER);
        cell.setPadding(2);
        return cell;
    }

    public static PdfPCell plain(String text, Font font) {
        return plain(new Phrase(text == null ? "" : text, font));
    }

    public static PdfPCell plainRight(String text, Font font) {
        PdfPCell cell = plain(text, font);
        cell.setHorizontalAlignment(Element.ALIGN_RIGHT);
        return cell;
    }

    /** Header cell of a data table: filled, bottom rule. */
    public static PdfPCell header(String text, int alignment) {
        PdfPCell cell = new PdfPCell(new Phrase(text, TABLE_HEADER));
        cell.setBackgroundColor(HEADER_FILL);
        cell.setBorder(Rectangle.BOTTOM);
        cell.setBorderColor(RULE);
        cell.setBorderWidth(0.6f);
        cell.setPadding(5);
        cell.setHorizontalAlignment(alignment);
        return cell;
    }

    /** Body cell of a data table: light bottom rule, optional stripe. */
    public static PdfPCell body(String text, int alignment, boolean striped) {
        PdfPCell cell = new PdfPCell(new Phrase(text == null ? "" : text, BODY));
        cell.setBorder(Rectangle.BOTTOM);
        cell.setBorderColor(RULE);
        cell.setBorderWidth(0.3f);
        cell.setPadding(4.5f);
        cell.setHorizontalAlignment(alignment);
        if (striped) {
            cell.setBackgroundColor(STRIPE_FILL);
        }
        return cell;
    }
}
