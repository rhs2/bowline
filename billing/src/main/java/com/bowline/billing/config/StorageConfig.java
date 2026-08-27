package com.bowline.billing.config;

import com.bowline.billing.storage.LocalPdfStore;
import com.bowline.billing.storage.PdfStore;
import com.bowline.billing.storage.S3PdfStore;
import java.net.URI;
import java.nio.file.Path;
import java.time.Clock;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;
import org.springframework.beans.factory.ObjectProvider;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Lazy;
import org.springframework.context.annotation.Primary;
import software.amazon.awssdk.auth.credentials.AwsBasicCredentials;
import software.amazon.awssdk.auth.credentials.StaticCredentialsProvider;
import software.amazon.awssdk.http.apache.ApacheHttpClient;
import software.amazon.awssdk.regions.Region;
import software.amazon.awssdk.services.s3.S3Client;
import software.amazon.awssdk.services.s3.S3ClientBuilder;

/** Chooses the PDF store from {@code BILLING_PDF_OUTPUT} and builds the S3 client when needed. */
@Configuration
public class StorageConfig {

    private static final Logger log = LoggerFactory.getLogger(StorageConfig.class);

    @Bean
    public Clock clock() {
        return Clock.systemUTC();
    }

    /**
     * Lazy so that local mode never instantiates an S3 client. With {@code S3_ENDPOINT}
     * set the client talks to MinIO (or any S3 compatible store); with it blank the SDK
     * uses the regional AWS endpoint. Static keys are optional: when blank the default
     * credential chain (task role, instance profile, environment) applies.
     */
    @Bean(destroyMethod = "close")
    @Lazy
    public S3Client s3Client(BillingProperties properties) {
        BillingProperties.S3 s3 = properties.s3();
        S3ClientBuilder builder = S3Client.builder()
                .region(Region.of(s3.region()))
                .httpClientBuilder(ApacheHttpClient.builder())
                .forcePathStyle(s3.forcePathStyle());
        if (s3.hasEndpointOverride()) {
            builder.endpointOverride(URI.create(s3.endpoint()));
        }
        if (s3.hasStaticCredentials()) {
            builder.credentialsProvider(StaticCredentialsProvider.create(
                    AwsBasicCredentials.create(s3.accessKeyId(), s3.secretAccessKey())));
        }
        log.info("S3 client: region={} endpoint={} pathStyle={} bucket={}",
                s3.region(), s3.hasEndpointOverride() ? s3.endpoint() : "aws", s3.forcePathStyle(), s3.bucket());
        return builder.build();
    }

    /**
     * Invoices and statements. Primary, because it is the store most of the service
     * means when it asks for a {@link PdfStore}.
     */
    @Bean
    @Primary
    public PdfStore pdfStore(BillingProperties properties, ObjectProvider<S3Client> s3Client) {
        return store(properties, s3Client, properties.s3().bucket(), "invoice PDF output");
    }

    /**
     * Personnel files. They have to land in the documents bucket, because that is where
     * the API presigns employee document downloads from; in local mode both stores share
     * one directory and the key prefixes keep them apart.
     */
    @Bean
    public PdfStore documentStore(BillingProperties properties, ObjectProvider<S3Client> s3Client) {
        return store(properties, s3Client, properties.s3().documentsBucket(), "employee document output");
    }

    private static PdfStore store(
            BillingProperties properties, ObjectProvider<S3Client> s3Client, String bucket, String what) {
        return switch (properties.pdfOutput()) {
            case LOCAL -> {
                LocalPdfStore store = new LocalPdfStore(Path.of(properties.localOutputDir()));
                log.info("{}: local directory {}", what, store.root());
                yield store;
            }
            case S3 -> {
                log.info("{}: s3 bucket {}", what, bucket);
                yield new S3PdfStore(s3Client.getObject(), bucket);
            }
        };
    }
}
