package com.bowline.billing.support;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Comparator;
import java.util.stream.Stream;

/**
 * One temp directory per test JVM for {@code BILLING_PDF_OUTPUT=local}. Shared by every
 * Spring context so the cached context never points at a directory a finished test
 * class already deleted. Removed on JVM exit.
 */
public final class TestOutput {

    public static final Path DIR = create();

    private TestOutput() {}

    private static Path create() {
        try {
            Path dir = Files.createTempDirectory("bowline-billing-test-");
            Runtime.getRuntime().addShutdownHook(new Thread(() -> deleteRecursively(dir), "test-output-cleanup"));
            return dir;
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
    }

    private static void deleteRecursively(Path dir) {
        try (Stream<Path> paths = Files.walk(dir)) {
            paths.sorted(Comparator.reverseOrder()).forEach(p -> {
                try {
                    Files.deleteIfExists(p);
                } catch (IOException ignored) {
                    // best effort cleanup of a temp directory
                }
            });
        } catch (IOException ignored) {
            // best effort cleanup of a temp directory
        }
    }
}
