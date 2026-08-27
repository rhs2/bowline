package com.bowline.billing.storage;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * Writes PDFs below a root directory ({@code BILLING_PDF_OUTPUT=local}). Keys are
 * confined to the root and files are written atomically (temp file then move).
 */
public class LocalPdfStore implements PdfStore {

    private static final Logger log = LoggerFactory.getLogger(LocalPdfStore.class);

    private final Path root;

    public LocalPdfStore(Path root) {
        this.root = root.toAbsolutePath().normalize();
    }

    public Path root() {
        return root;
    }

    @Override
    public StoredPdf store(String key, byte[] bytes) {
        Path target = root.resolve(key).normalize();
        if (!target.startsWith(root)) {
            throw new IllegalArgumentException("key escapes the output directory: " + key);
        }
        try {
            Files.createDirectories(target.getParent());
            Path temp = Files.createTempFile(target.getParent(), ".partial-", ".pdf");
            try {
                Files.write(temp, bytes);
                Files.move(temp, target, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.ATOMIC_MOVE);
            } finally {
                Files.deleteIfExists(temp);
            }
        } catch (IOException e) {
            throw new StorageException("could not write " + target, e);
        }
        log.debug("wrote {} ({} bytes)", target, bytes.length);
        return new StoredPdf(key, bytes.length);
    }
}
