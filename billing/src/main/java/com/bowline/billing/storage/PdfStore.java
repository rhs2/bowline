package com.bowline.billing.storage;

/** Where rendered PDFs end up: S3 (or MinIO) in real deployments, a directory in tests. */
public interface PdfStore {

    /** A stored document: the key the API records on the invoice and the byte count. */
    record StoredPdf(String key, long bytes) {}

    /**
     * Persist {@code bytes} under {@code key} (for example {@code invoices/INV-2026-000123.pdf}),
     * replacing any previous object with that key.
     *
     * @throws StorageException when the backend rejects the write
     */
    StoredPdf store(String key, byte[] bytes);
}
