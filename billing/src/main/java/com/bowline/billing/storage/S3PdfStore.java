package com.bowline.billing.storage;

import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import software.amazon.awssdk.core.exception.SdkException;
import software.amazon.awssdk.core.sync.RequestBody;
import software.amazon.awssdk.services.s3.S3Client;
import software.amazon.awssdk.services.s3.model.PutObjectRequest;

/** Uploads PDFs to the {@code S3_BUCKET_PDFS} bucket ({@code BILLING_PDF_OUTPUT=s3}). */
public class S3PdfStore implements PdfStore {

    private static final Logger log = LoggerFactory.getLogger(S3PdfStore.class);

    private final S3Client client;
    private final String bucket;

    public S3PdfStore(S3Client client, String bucket) {
        this.client = client;
        this.bucket = bucket;
    }

    @Override
    public StoredPdf store(String key, byte[] bytes) {
        PutObjectRequest request = PutObjectRequest.builder()
                .bucket(bucket)
                .key(key)
                .contentType("application/pdf")
                .contentLength((long) bytes.length)
                .build();
        try {
            client.putObject(request, RequestBody.fromBytes(bytes));
        } catch (SdkException e) {
            throw new StorageException("could not upload s3://" + bucket + "/" + key, e);
        }
        log.debug("uploaded s3://{}/{} ({} bytes)", bucket, key, bytes.length);
        return new StoredPdf(key, bytes.length);
    }
}
