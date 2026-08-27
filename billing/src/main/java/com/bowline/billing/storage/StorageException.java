package com.bowline.billing.storage;

/** The PDF could be rendered but not persisted (S3 or filesystem failure). */
public class StorageException extends RuntimeException {

    public StorageException(String message, Throwable cause) {
        super(message, cause);
    }
}
