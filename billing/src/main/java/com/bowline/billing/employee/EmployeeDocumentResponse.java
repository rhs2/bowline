package com.bowline.billing.employee;

/** Response of {@code POST /render/document}: {@code {"s3_key": "...", "bytes": 12345}}. */
public record EmployeeDocumentResponse(String s3Key, long bytes) {}
