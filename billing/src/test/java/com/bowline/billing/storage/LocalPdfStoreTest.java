package com.bowline.billing.storage;

import static org.assertj.core.api.Assertions.assertThat;
import static org.assertj.core.api.Assertions.assertThatThrownBy;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class LocalPdfStoreTest {

    @Test
    void writesBelowTheRootAndReportsTheSize(@TempDir Path root) throws Exception {
        LocalPdfStore store = new LocalPdfStore(root);
        byte[] bytes = "%PDF-1.4 test".getBytes(StandardCharsets.US_ASCII);

        PdfStore.StoredPdf stored = store.store("invoices/INV-1.pdf", bytes);

        assertThat(stored.key()).isEqualTo("invoices/INV-1.pdf");
        assertThat(stored.bytes()).isEqualTo(bytes.length);
        assertThat(Files.readAllBytes(root.resolve("invoices/INV-1.pdf"))).isEqualTo(bytes);
    }

    @Test
    void refusesKeysThatEscapeTheRoot(@TempDir Path root) {
        LocalPdfStore store = new LocalPdfStore(root);
        assertThatThrownBy(() -> store.store("../outside.pdf", new byte[] {1}))
                .isInstanceOf(IllegalArgumentException.class);
        assertThat(root.resolveSibling("outside.pdf")).doesNotExist();
    }
}
