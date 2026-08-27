package com.bowline.billing.invoice;

import com.bowline.billing.storage.PdfStore;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.stereotype.Service;

/** Validates, renders and (unless previewing) stores an invoice PDF. */
@Service
public class InvoiceRenderService {

    private static final Logger log = LoggerFactory.getLogger(InvoiceRenderService.class);

    private final InvoicePdfRenderer renderer;
    private final PdfStore store;

    public InvoiceRenderService(InvoicePdfRenderer renderer, PdfStore store) {
        this.renderer = renderer;
        this.store = store;
    }

    /** The object key an invoice is stored under. */
    public static String keyFor(String invoiceNo) {
        return "invoices/" + invoiceNo + ".pdf";
    }

    /** Render only; nothing is written anywhere. */
    public byte[] preview(InvoiceRenderRequest request) {
        InvoiceRules.check(request);
        byte[] pdf = renderer.render(request);
        log.info("previewed invoice {} ({} bytes)", request.invoice().invoiceNo(), pdf.length);
        return pdf;
    }

    /** Render and persist under {@code invoices/<invoice_no>.pdf}. */
    public InvoiceRenderResponse renderAndStore(InvoiceRenderRequest request) {
        InvoiceRules.check(request);
        String invoiceNo = request.invoice().invoiceNo();
        byte[] pdf = renderer.render(request);
        PdfStore.StoredPdf stored = store.store(keyFor(invoiceNo), pdf);
        log.info("rendered invoice {} -> {} ({} bytes)", invoiceNo, stored.key(), stored.bytes());
        return new InvoiceRenderResponse(stored.key(), stored.bytes());
    }
}
