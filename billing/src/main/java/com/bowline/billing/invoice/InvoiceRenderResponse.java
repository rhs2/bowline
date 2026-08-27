package com.bowline.billing.invoice;

/** Response of {@code POST /render/invoice}: {@code {"s3_key": "...", "bytes": 12345}}. */
public record InvoiceRenderResponse(String s3Key, long bytes) {}
