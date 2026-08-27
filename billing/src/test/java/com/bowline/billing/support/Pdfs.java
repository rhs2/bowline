package com.bowline.billing.support;

import java.io.IOException;
import java.io.UncheckedIOException;
import org.apache.pdfbox.Loader;
import org.apache.pdfbox.pdmodel.PDDocument;
import org.apache.pdfbox.text.PDFTextStripper;

/** Reads rendered PDFs back with PDFBox so tests can assert on their content. */
public final class Pdfs {

    private Pdfs() {}

    public static boolean isPdf(byte[] bytes) {
        return bytes != null && bytes.length > 4
                && bytes[0] == '%' && bytes[1] == 'P' && bytes[2] == 'D' && bytes[3] == 'F';
    }

    public static String text(byte[] bytes) {
        try (PDDocument document = Loader.loadPDF(bytes)) {
            PDFTextStripper stripper = new PDFTextStripper();
            stripper.setSortByPosition(true);
            return stripper.getText(document);
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    /** Text with every run of whitespace collapsed, so assertions survive line wraps. */
    public static String flatText(byte[] bytes) {
        return text(bytes).replaceAll("\\s+", " ").trim();
    }

    public static int pages(byte[] bytes) {
        try (PDDocument document = Loader.loadPDF(bytes)) {
            return document.getNumberOfPages();
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }
}
