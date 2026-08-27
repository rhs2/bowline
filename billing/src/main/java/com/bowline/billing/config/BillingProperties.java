package com.bowline.billing.config;

import jakarta.validation.Valid;
import jakarta.validation.constraints.NotBlank;
import jakarta.validation.constraints.NotNull;
import java.time.Duration;
import org.springframework.boot.context.properties.ConfigurationProperties;
import org.springframework.validation.annotation.Validated;

/**
 * Service configuration bound from the {@code billing.*} keys in application.yml, which
 * in turn map to the {@code BILLING_*}, {@code S3_*} and {@code INTERNAL_SERVICE_TOKEN}
 * environment variables. The service refuses to start without an internal token.
 */
@ConfigurationProperties(prefix = "billing")
@Validated
public record BillingProperties(
        @NotBlank String internalToken,
        @NotNull PdfOutput pdfOutput,
        @NotBlank String localOutputDir,
        @NotNull Duration readinessTimeout,
        @Valid @NotNull Company company,
        @Valid @NotNull S3 s3) {

    /** Where rendered PDFs are written. */
    public enum PdfOutput {
        S3,
        LOCAL
    }

    /** The issuing company shown in document headers. */
    public record Company(@NotBlank String name, @NotBlank String address) {}

    /**
     * S3 (or MinIO) settings for {@link PdfOutput#S3}. Rendered invoices and statements
     * go to {@code bucket} ({@code S3_BUCKET_PDFS}); personnel files go to
     * {@code documentsBucket} ({@code S3_BUCKET_DOCUMENTS}), the bucket the API
     * presigns employee document downloads from.
     */
    public record S3(
            String endpoint,
            @NotBlank String region,
            @NotBlank String bucket,
            @NotBlank String documentsBucket,
            String accessKeyId,
            String secretAccessKey,
            boolean forcePathStyle) {

        public boolean hasEndpointOverride() {
            return endpoint != null && !endpoint.isBlank();
        }

        public boolean hasStaticCredentials() {
            return accessKeyId != null && !accessKeyId.isBlank()
                    && secretAccessKey != null && !secretAccessKey.isBlank();
        }
    }
}
