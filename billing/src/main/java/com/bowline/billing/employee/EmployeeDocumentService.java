package com.bowline.billing.employee;

import com.bowline.billing.storage.PdfStore;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.annotation.Qualifier;
import org.springframework.stereotype.Service;

/**
 * Validates, renders and (unless previewing) stores a personnel file. Unlike an
 * invoice the key is not derived here: the row in {@code employee_documents} already
 * names the object, so the caller supplies it and the request validation keeps it
 * inside the employee document prefix.
 */
@Service
public class EmployeeDocumentService {

    private static final Logger log = LoggerFactory.getLogger(EmployeeDocumentService.class);

    private final EmployeeDocumentPdfRenderer renderer;
    private final PdfStore store;

    public EmployeeDocumentService(
            EmployeeDocumentPdfRenderer renderer, @Qualifier("documentStore") PdfStore store) {
        this.renderer = renderer;
        this.store = store;
    }

    /** Render only; nothing is written anywhere. */
    public byte[] preview(EmployeeDocumentRequest request) {
        EmployeeDocumentKind kind = EmployeeDocumentRules.check(request);
        byte[] pdf = renderer.render(kind, request);
        log.info("previewed {} for {} ({} bytes)", kind.wireName(), request.employee().employeeNo(), pdf.length);
        return pdf;
    }

    /** Render and persist under the key the caller asked for. */
    public EmployeeDocumentResponse renderAndStore(EmployeeDocumentRequest request) {
        EmployeeDocumentKind kind = EmployeeDocumentRules.check(request);
        byte[] pdf = renderer.render(kind, request);
        PdfStore.StoredPdf stored = store.store(request.s3Key(), pdf);
        log.info("rendered {} for {} -> {} ({} bytes)",
                kind.wireName(), request.employee().employeeNo(), stored.key(), stored.bytes());
        return new EmployeeDocumentResponse(stored.key(), stored.bytes());
    }
}
