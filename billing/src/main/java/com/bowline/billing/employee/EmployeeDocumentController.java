package com.bowline.billing.employee;

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
 * {@code POST /render/document}: renders one personnel file (contract, payslip,
 * certificate or identity record), stores it under the key the caller gave and answers
 * with that key. With {@code ?inline=1} the bytes come back as {@code application/pdf}
 * and nothing is stored, which is what a preview wants.
 */
@RestController
@RequestMapping("/render")
public class EmployeeDocumentController {

    private final EmployeeDocumentService service;

    public EmployeeDocumentController(EmployeeDocumentService service) {
        this.service = service;
    }

    @PostMapping(value = "/document", consumes = MediaType.APPLICATION_JSON_VALUE)
    public ResponseEntity<?> render(
            @Valid @RequestBody EmployeeDocumentRequest request,
            @RequestParam(name = "inline", required = false) String inline) {
        if (Flags.truthy(inline)) {
            byte[] pdf = service.preview(request);
            return ResponseEntity.ok()
                    .contentType(MediaType.APPLICATION_PDF)
                    .header(HttpHeaders.CONTENT_DISPOSITION, ContentDisposition.inline()
                            .filename(fileName(request.s3Key())).build().toString())
                    .body(pdf);
        }
        return ResponseEntity.ok().contentType(MediaType.APPLICATION_JSON).body(service.renderAndStore(request));
    }

    /** The last segment of the object key, which the request pattern guarantees is a PDF. */
    private static String fileName(String s3Key) {
        return s3Key.substring(s3Key.lastIndexOf('/') + 1);
    }
}
