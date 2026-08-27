package com.bowline.billing.web;

/** The referenced entity does not exist; rendered as a 404 {@code not_found} problem. */
public class NotFoundException extends RuntimeException {

    public NotFoundException(String message) {
        super(message);
    }
}
