package com.bowline.billing.invoice;

import com.bowline.billing.web.Flags;
import jakarta.validation.Valid;
import org.springframework.http.ContentDisposition;
import org.springframework.http.HttpHeaders;
import org.springframework.http.MediaType;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;

/**
 * {@code POST /render/invoice}: renders the PDF and stores it, answering with the key.
 * With {@code ?inline=1} the bytes are returned as {@code application/pdf} instead and
 * nothing is stored, which is what a preview wants.
 */
@RestController
@RequestMapping("/render")
public class InvoiceController {

    private final InvoiceRenderService service;

    public InvoiceController(InvoiceRenderService service) {
        this.service = service;
    }

    @PostMapping(value = "/invoice", consumes = MediaType.APPLICATION_JSON_VALUE)
    public ResponseEntity<?> render(
            @Valid @RequestBody InvoiceRenderRequest request,
            @RequestParam(name = "inline", required = false) String inline) {
        if (Flags.truthy(inline)) {
            byte[] pdf = service.preview(request);
            return ResponseEntity.ok()
                    .contentType(MediaType.APPLICATION_PDF)
                    .header(HttpHeaders.CONTENT_DISPOSITION, ContentDisposition.inline()
                            .filename(request.invoice().invoiceNo() + ".pdf").build().toString())
                    .body(pdf);
        }
        return ResponseEntity.ok().contentType(MediaType.APPLICATION_JSON).body(service.renderAndStore(request));
    }
}
